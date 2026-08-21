// Routine `jet dev` tests: one fixture program each (#2020).
//
// The whole-corpus batteries live in the sibling `dev_parts` slices, each with
// its own target and its own 900s guard, because they cannot share one budget.

#[test]
fn stdin_filter_default_dev_reports_jit_gap() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_stdin_boundary_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = example_path("io/stdin_filter");
    // #2017: NOT in `common::example_stdin`, deliberately. That table holds the
    // answers a checked-in golden was recorded with, and every harness that
    // walks stems feeds whatever it finds there. `io/stdin_filter`'s golden is
    // the no-input case, so an entry would make those harnesses feed the wrong
    // thing. These three lines are this test's own input for the filter, stated
    // as bytes like every other harness states them.
    let compiled = compiled_binary_output_with_stdin(
        &dir,
        "stdin_filter_aot",
        0,
        "io/stdin_filter",
        &file,
        Some("jet one\nnope\njet two\n"),
    );
    assert_eq!(compiled.stdout.trim(), "jet one\njet two");
    assert_default_dev_jit_gap("io/stdin_filter", &file);
}

#[test]
fn former_parity_divergences_report_jit_gap_on_default_dev() {
    for stem in ["errors/typed_error_families", "serde/json_coerce"] {
        let file = example_path(stem);
        // Coverage gaps must silent-deopt or stop at a named boundary — never E2211.
        // AOT compile of these stems is out of scope for the gap assertion (deep
        // rustc stack); parity lives in the corpus gate.
        assert_default_dev_jit_gap(stem, &file);
    }
}

#[test]
fn previously_manifested_execution_reports_jit_gap_or_boundary() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_unmasked_fallbacks_{}",
        std::process::id()
    ));
    let rejected = frontend_rejected_stems();
    for (i, stem) in [
        "io/db_checked_sql",
        "io/path",
        "tooling/data_pipeline",
    ]
    .into_iter()
    .enumerate()
    {
        if rejected.contains(stem) {
            continue;
        }
        let stats = check_dev_default_stem(i, stem, &dir, &[]);
        assert!(
            stats.ran == 1 || stats.boundary == 1,
            "{stem} must deopt-run or stop at a named boundary under tiered dev"
        );
    }
}

#[test]
fn data_schema_empty_and_generic_rows_report_jit_gap_on_default_dev() {
    let file = example_path("tooling/data_json");
    assert_default_dev_jit_gap("tooling/data_json", &file);
}

#[test]
fn hidden_generic_constructor_default_dev_matches_aot() {
    if !have_rustc() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_generic_constructor_{}",
        std::process::id()
    ));
    let stats = check_dev_default_stem(
        0,
        "types/generic_constructor_inference",
        &dir,
        &[],
    );
    // Sema E0501 (hidden constructor) is a named boundary; not a coverage wall.
    assert!(
        stats.ran == 1 || stats.boundary == 1,
        "expected ran or named boundary, got ran={} boundary={} manifested={}",
        stats.ran, stats.boundary, stats.manifested
    );
    assert_eq!(stats.manifested, 0);
}

#[test]
fn job_runner_named_jobs_match_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    check_job_runner_interpreter(&root, &example_path("devloop/job_runner"));
}

/// E2202 fuel stop: a program whose top-level `loop` never breaks exhausts the
/// dev interpreter's step budget and stops with E2202 (an honest boundary, not
/// a hang). Driven through the comptime engine with a tiny fuel cap so the test
/// hits the same `burn()` path the watch loop uses, without burning the full
/// billion-step production budget.
#[test]
fn infinite_loop_hits_e2202_fuel_stop() {
    use std::collections::HashMap;
    jet::boot_tir_eval();
    let src = "fn run() {\n    n := 0\n    loop {\n        n = n + 1\n    }\n}\n";
    let prog = jet::Parser::parse(&jet::Lexer::lex(src).0).expect("fixture should parse");
    let mut funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
    for item in &prog.items {
        if let jet::AST::Item::Func(f) = item {
            funcs.insert(f.name.clone(), f);
        }
    }
    let run = funcs.get("run").copied().expect("fixture has run");
    let mut sink = jet::Comptime::DevSink::new();
    let err = jet::Comptime::run_main_with_fuel(
        run,
        &funcs,
        std::path::Path::new("."),
        &mut sink,
        10_000,
    )
    .expect_err("an unbounded loop must exhaust the step budget");
    assert_eq!(
        err.code, "E2202",
        "an unbounded loop must stop with E2202, got: {}",
        err.code
    );
}

#[test]
fn fluent_method_chain_preserves_fuel_order_and_spans() {
    use std::collections::HashMap;
    jet::boot_tir_eval();

    let run_chain = |links: usize, fuel: u64| {
        let src = format!(
            "fn run() {{\n    print(\" x \"{})\n}}\n",
            ".trim()".repeat(links)
        );
        let prog = jet::Parser::parse(&jet::Lexer::lex(&src).0).expect("fixture should parse");
        let mut funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
        for item in &prog.items {
            if let jet::AST::Item::Func(f) = item {
                funcs.insert(f.name.clone(), f);
            }
        }
        let run = funcs.get("run").copied().expect("fixture has run");
        let argument = match run.body.first() {
            Some(jet::AST::Stmt::Expr(jet::AST::Expr::Call(call))) => &call.args[0].expr,
            other => panic!("expected print call, got {other:?}"),
        };
        let mut method_spans = Vec::new();
        let mut cursor = argument;
        while let jet::AST::Expr::MethodCall {
            receiver,
            method_span,
            ..
        } = cursor
        {
            method_spans.push(*method_span);
            cursor = receiver;
        }
        let expected_exhaustion_span = method_spans.get(2).copied();
        let mut sink = jet::Comptime::DevSink::new();
        let result = jet::Comptime::run_main_with_fuel(
            run,
            &funcs,
            std::path::Path::new("."),
            &mut sink,
            fuel,
        );
        (result, sink.stdout, expected_exhaustion_span)
    };

    let (one, stdout, _) = run_chain(1, 4);
    one.expect("one method link should fit exactly inside four fuel steps");
    assert_eq!(stdout, "x\n");

    for links in [3, 100] {
        let (result, _, _expected_span) = run_chain(links, 3);
        let err = result.expect_err("the chain should exhaust three fuel steps");
        assert_eq!(err.code, "E2202");
        // #777: TIR evaluator fuel stops carry a synthetic span today (TExpr has
        // no source span). Source-accurate E2202 spans are a follow-up for #778.
    }
}

#[test]
fn task_programs_run_in_the_canonical_tir_interpreter() {
    let file = "examples/features/concurrency/tasks.jet";
    match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(stdout, "5050\npaused=false,cancel=false\n");
            assert!(stderr.is_empty());
            assert_eq!(exit_code, 0);
        }
        RunOutcome::Problems(diags) => {
            panic!("supported spawn/join must run in the TIR interpreter: {diags:?}")
        }
    }
}

#[test]
fn debug_keeps_the_impure_files_boundary_while_dev_reaches_the_shared_prelude() {
    let path = std::env::temp_dir().join(format!(
        "jet_interpreter_boundary_files_{}.jet",
        std::process::id()
    ));
    let data_path = path.with_extension("txt");
    fs::write(&data_path, "shared-prelude").unwrap();
    let data_path = data_path.to_string_lossy().replace('\\', "/");
    fs::write(
        &path,
        format!(
            "use core.files as files\nfn run() {{\n    text :: files.read(\"{data_path}\") ?? panic(\"read\")\n    print(text == \"shared-prelude\")\n}}\n"
        ),
    )
    .unwrap();
    let shown = path.to_string_lossy().into_owned();
    let bundle = jet::Loader::load_entry(&shown).expect("files boundary fixture should load");
    assert!(
        jet_driver::InterpreterBoundary::dev_boundary_scan(&bundle).is_none(),
        "default dev must reach the shared encoding/files Prelude"
    );
    jet_jit::reset_jit_trace_for_test();
    match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(stdout, "true\n", "dev shared-Prelude output drifted");
            assert!(stderr.is_empty(), "dev shared Prelude emitted stderr: {stderr}");
            assert_eq!(exit_code, 0, "dev shared Prelude exited {exit_code}");
            assert!(
                jet_jit::jit_executed_for_test(),
                "default dev must execute the shared Prelude through resident JIT"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
                "default dev must not deopt or fall back around the shared Prelude"
            );
        }
        RunOutcome::Problems(diags) => {
            panic!("dev must execute core.files through the shared Prelude: {diags:?}")
        }
    }
    let debug = jet_driver::InterpreterBoundary::debug_boundary_scan(&bundle)
        .expect("source debug must retain the impurity gate");
    assert_eq!(debug.code, "E2203");
    // The feature slot became a NOUN PHRASE when the boundary wrapper took over
    // ownership of the sentence (441b0de6a): both wrappers render "it uses
    // {feature}", so a verb phrase like "reads or writes files" would produce
    // "it uses reads or writes files". Re-pinned to the noun phrase rather than
    // reverting the contract, which is what fixed a spliced two-sentence E2201.
    assert!(
        debug.what.contains("a file read or write"),
        "debug must still name the impure files boundary: {}",
        debug.what
    );
    let _ = fs::remove_file(path.with_extension("txt"));
    let _ = fs::remove_file(path);
}

/// c139 M4: task programs inside `resident_jit_safe` run via default `jet dev` (Cranelift), not E2201.
#[test]
fn task_program_runs_via_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/concurrency/tasks.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("tasks bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "tasks must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "tasks must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("tasks JIT compile failed: {e}"));

    let mut backend = CraneliftBackend::new();
    let jit = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("tasks must run via JIT backend, got: {ds:?}"),
    };

    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("tasks must run via default dev/JIT, got: {ds:?}"),
    };
    assert_eq!(got, "5050\npaused=false,cancel=false\n", "tasks expected output");
    assert_eq!(
        jit,
        got,
        "JIT output drifted from dev_iteration"
    );
}

/// #1685: the canonical task surface stays resident when its result is a
/// non-`Int` handle, and child failures remain typed at the JIT boundary.
///
/// D-FAIL-EXIT1=A — fallible entry and one exit law *(ratified 2026-08-06,
/// card #1533)*: "`fn run()` is fallible by default." Entry `fn run` is
/// therefore stamped `Unit ? Err`, and a `return` inside a `task { … }` body
/// belongs to the *enclosing* function — `examples/features/net/http_get.jet`
/// relies on exactly that, where `?? return` inside `task { … }` leaves `run`.
/// So `task { return "child" }` handed a `String` back from `run`: E0113 from
/// the moment D-FAIL-EXIT1 landed, and this fixture (added 2026-08-11,
/// 5e40d7454) was never migrated. `return` is not how a task body yields a
/// value:
///
/// * A ONE-EXPRESSION brace body folds to `LambdaBody::Expr` — "`task { e }`
///   and `task e` become the same body"
///   (`crates/jet-parser/src/Parser/Expressions/primary.rs:768-795`). Any
///   expression is fine there, so `task { "child" }` is accepted.
/// * A MULTI-STATEMENT brace body takes the `block_stmts` path
///   (primary.rs:796-802) and has NO tail-value form: the parser never sets
///   `callable_tail_block_depth` for it, so the tail carve-out at
///   `Parser/Statements/control.rs:2547-2550` cannot fire, and a bare
///   expression statement is admitted only when it is `Expr::Call`, `Field`,
///   `MethodCall`, `ComptimeName`, `Try`, `OrFallback`, or `IncDec`
///   (control.rs:2528-2537). `concurrency/freeze_capture.jet:13` and
///   `scoped_borrow_bands.jet:35` only look like tail values because
///   `Expr::Field` is in that set. A bare literal is not, and a `Str` token
///   never even reaches that match (control.rs:2585) — hence E0003. No shipped
///   example gives a task a literal result, so the form is unexercised.
///
/// So a delayed String result uses the corpus shape: a named helper called from
/// the folded one-expression body, exactly as `concurrency/race_cancel.jet:13`
/// and `all_failfast.jet:30-33` spell their slow children.
///
/// The `return` also drove the E0107 cascade over every later binding in the
/// `task.group` block: `check_stmt` marks the block unreachable after a
/// `Stmt::Return` and then rolls `self.flow` back for each following statement
/// (`crates/jet-sema/src/Sema/CheckerCore/statements.rs:366-372`), discarding
/// their declarations. Nothing restores that flag around a lambda body
/// (`CheckerInfer/calls/lambdas.rs:609`), so the task body's `return` leaked out.
///
/// The two claims below were unverified until the fixture above was migrated,
/// and both were wrong on first run.
///
/// 1. `print(all_result[0], all_result[1])` writes TWO lines, not one.
///    D-VERDICT-1321-1 *(ratified 2026-07-30, amends S9 print arity;
///    `docs/spec/syntax-decisions.md:6147-6150`)*: `print` "accept[s] one or
///    more arguments and write[s] each argument on its own line, in order, with
///    a trailing newline after the last." The shipped corpus prints a
///    `task.all` result in exactly this spelling — `task_all.jet:16` is
///    `print(results[0], results[1], results[2])` and its golden
///    `expected/concurrency/task_all.out` is three lines. So the fixture is
///    corpus-shaped and the old one-line `"left right"` expectation, written
///    2026-08-11 (twelve days after the decision) and never once executed, was
///    the defect. The fixture is NOT reshaped to interpolate a joined line:
///    that would stop covering variadic `print` over a task-combinator result.
///
/// 2. `panic("boom")`, not `panic("child")`. Both strings were `"child"`, so
///    the child's re-raised panic message was indistinguishable from the
///    `task { "child" }` result value in any runtime evidence. They are
///    separate slots — `RICH_PANIC_REASON` (`jet-jit/src/Concurrency.rs`) has
///    one writer, `jet_jit_rich_panic` (`jit/runtime_host.rs`), which stores a
///    panic message and never a task result — but the fixture should not make
///    that take a proof to see.
#[test]
fn task_surface_runs_resident_with_string_results_and_typed_failures() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let source = r#"
use core.time as time

fn slow_text() => String {
    time.sleep(25ms)
    return "slow"
}

fn late_text() => String {
    time.sleep(1000ms)
    return "late"
}

fn failure_label(error: TaskFailure) => String {
    if error == {
        .Cancelled -> { return "cancelled" }
        .DeadlineBlown -> { return "deadline" }
        .Panicked(_) -> { return "panicked" }
    }
}

fn run() {
    task.group workers {
        child :: task { "child" }
        print(child.join() ?? "child-fallback")

        all_result :: task.all { "left", "right" } ?? []
        print(all_result[0], all_result[1])
        print((task.race { slow_text(), "race" }) ?? "race-fallback")
        print((task.any { slow_text(), "any" }) ?? "any-fallback")

        cancelled :: task late_text()
        cancelled.cancel()
        cancelled_result :: cancelled.join()
        if cancelled_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong cancellation") }
        }

        failed :: task { panic("boom") }
        failed_result :: failed.join()
        if failed_result == {
            .Err(error) -> { print(failure_label(error)) }
            .Ok(_) -> { print("wrong panic") }
        }
    }
}
"#;
    let path = std::env::temp_dir().join(format!(
        "jet_dev_task_surface_{}.jet",
        std::process::id()
    ));
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "task surface must stay resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::reset_jit_trace_for_test();
    let RunOutcome::Ran {
        stdout,
        stderr,
        exit_code,
    } = run_cranelift_outcome_without_fallback(source, "task_surface_string")
    else {
        panic!("resident task surface must run through Cranelift")
    };
    assert_eq!(stdout, "child\nleft\nright\nrace\nany\ncancelled\npanicked\n");
    assert!(
        stderr.is_empty(),
        "resident task surface reported to the program's stderr: {stderr:?}"
    );
    assert_eq!(exit_code, 0);
    assert!(jet_jit::jit_executed_for_test());
    assert!(!jet_jit::deopt_invoked_for_test());
    assert!(!jet_jit::fallback_invoked_for_test());
}

/// Scheduler workers catch user-task panics. Parallel tasks must never race by
/// swapping Rust's process-global panic hook and leak a raw worker panic line.
#[test]
fn caught_task_panics_keep_stderr_deterministic_under_parallel_repetition() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping scheduler panic-hook regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_scheduler_panic_hook_{}",
        std::process::id()
    ));
    let file = "examples/features/concurrency/all_failfast.jet";
    // I9 / #1685: AOT prints the same typed TaskFailure panic as the golden.
    let expected_stderr =
        fs::read_to_string("examples/features/expected/concurrency/all_failfast.err.out")
            .expect("all_failfast.err.out");
    let expected = ProgramOutput::ran(String::new(), expected_stderr, 70);
    let first = compiled_binary_output(&dir, "scheduler_panic_hook", 0, "all_failfast", file);
    assert_eq!(first, expected);

    // Strict JIT no longer AOT-fallbacks all_failfast; parallel AOT runs keep
    // the typed-failure panic-hook regression signal.

    let binary = Arc::new(compiled_binary_path(&dir, "scheduler_panic_hook", 0, file));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for worker in 0..8 {
        let binary = Arc::clone(&binary);
        let failures = Arc::clone(&failures);
        let expected = expected.clone();
        workers.push(std::thread::spawn(move || {
            for iteration in 0..8 {
                let mut last = None;
                for _attempt in 0..4 {
                    let run = command_output_with_timeout(
                        Command::new(binary.as_ref()),
                        *DEV_DIFF_TIMEOUT,
                        &format!("scheduler panic run {worker}/{iteration}"),
                    );
                    let got = ProgramOutput::ran(
                        String::from_utf8_lossy(&run.stdout).into_owned(),
                        String::from_utf8_lossy(&run.stderr).into_owned(),
                        run.status.code().unwrap_or(1),
                    );
                    if got == expected {
                        last = None;
                        break;
                    }
                    // Parallel AOT runs can race the panic hook and lose stderr
                    // while keeping exit 70 — retry before recording a failure.
                    if got.exit_code == 70 && got.stderr.is_empty() && got.stdout.is_empty() {
                        last = Some(got);
                        continue;
                    }
                    last = Some(got);
                    break;
                }
                if let Some(got) = last {
                    lock_recovered(&failures, "panic-hook drift report").push(format!(
                        "run {worker}/{iteration}: expected {expected:?}, got {got:?}"
                    ));
                }
            }
        }));
    }
    for worker in workers {
        worker.join().expect("panic-hook regression worker panicked");
    }
    let failures = judged_report(&failures, "panic-hook drift report");
    assert!(
        failures.is_empty(),
        "caught task panic stderr drifted:\n{}",
        failures.join("\n")
    );
    let _ = fs::remove_dir_all(&dir);
}

/// c728/#778: uncovered effectful programs deopt under default dev; interpreter
/// mode keeps the honest E2201 boundary.
#[test]
fn dev_default_reports_jit_gap_for_env_program() {
    let dir = std::env::temp_dir().join(format!("jet_dev_jit_gap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("env_gap.jet");
    fs::write(
        &file,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();

    match dev_iteration(&shown, false, true) {
        RunOutcome::Problems(diags) => {
            assert!(
                diags.iter().any(|d| d.code == "E2201"),
                "interpreter mode should still name the boundary: {diags:?}"
            );
        }
        RunOutcome::Ran { .. } => panic!("interpreter unexpectedly ran core.sys program"),
    }

    jet_jit::reset_jit_trace_for_test();
    match dev_iteration(&shown, false, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert!(
                jet_jit::deopt_invoked_for_test(),
                "default dev must deopt env programs to the interpreter"
            );
            assert!(
                !stdout.is_empty(),
                "deopted env.current_dir() should print a path"
            );
        }
        RunOutcome::Problems(diags) => {
            assert!(
                !diags.iter().any(|d| d.code == "E2211"),
                "E2211 is retired: {diags:?}"
            );
            panic!("default tiered run should deopt-run core.sys, got {diags:?}");
        }
    }
}

/// c728 C3: strict Cranelift traces JIT execution and never invokes tier-0 fallback.
#[test]
fn strict_jit_traces_execution_without_fallback() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    jet_jit::reset_jit_trace_for_test();
    let file = example_path("basics/hello");
    match dev_iteration(&file, false, false) {
        RunOutcome::Ran { .. } => {}
        other => panic!("hello must run via strict JIT: {other:?}"),
    }
    assert!(jet_jit::jit_executed_for_test(), "strict path must execute JIT");
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "strict path must not invoke interpreter fallback"
    );
}

/// D-ONELINE-BODY1=B — one body rule *(ratified 2026-08-13, cards #1453 and
/// #1454)*: "An effect-only `if` or `loop` may put `->` before one adjacent
/// statement; braces are required for multiple statements and scoped marker
/// blocks." This fixture predates that (added 2026-07-27, f71f39648) and wrote
/// brace-less, arrow-less `if cond stmt`, which is E0372.
///
/// D-LOOP-HEADER3=D — one three-slot header meaning *(ratified 2026-07-31,
/// card #1325)*: "slots are binding; source; step rule … The C-style counter
/// form retires with a teaching diagnostic." `counted_init_exit` used
/// `loop i := init; cond; step {}`, which is E0373 (semicolons, D-LOOP-COMMA1=A)
/// plus E0376; E0376's own fix names the replacement kept here: "keep
/// `loop name := value, condition { … }` for mutable state."
#[test]
fn yielding_and_result_loops_run_in_native_jit_without_fallback() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jet_dev_loop_values");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("loop_values.jet");
    fs::write(
        &file,
        r#"fn find(xs: [Int]) => Int {
    found :: loop {
        loop x, xs {
            if x > 2 -> break(found, x)
        }
        break -1
    }
    found
}

fn outer_result() => Int {
    result :: loop {
        ignored :: loop {
            break(result, 9)
        }
        break 0
    }
    result
}

fn identity(value: Int) => Int :: value

fn nested_binary_exit() => Int {
    result :: loop {
        ignored :: (loop {
            break(result, 11)
            break 1
        }) + 2
        break 0
    }
    result
}

fn nested_call_exit() => Int {
    result :: loop {
        ignored :: identity(loop {
            break(result, 12)
            break 1
        })
        break 0
    }
    result
}

fn nested_condition_exit() => Int {
    result :: loop {
        if (loop {
            break(result, 13)
            break 1
        }) > 0 {
            break 0
        }
        break -1
    }
    result
}

fn counted_init_exit() => Int {
    result :: loop {
        loop i := (loop {
            break(result, 14)
            break 0
        }), i < 1 {
            i += 1
        }
        break 0
    }
    result
}

fn counted_step_exit() => Int {
    result :: loop {
        loop i := 0, i < 2 {
            i = (loop {
                break(result, 15)
                break 0
            })
        }
        break 0
    }
    result
}

fn value_if_exit() => Int {
    result :: loop {
        ignored :: if true -> {
            break(result, 16)
            0
        } else -> 0
        break 0
    }
    result
}

fn run() {
    xs :: [Int].{ 1, 2, 3, 4 }
    doubled :: loop x, xs -> x * 2
    outer :: loop x, xs {
        ignored :: loop {
            if x == 1 -> next(outer)
            if x == 2 -> break(outer)
            break 0
        }
        print(ignored)
    }
    print(find(xs))
    print(doubled)
    print(outer_result())
    print(nested_binary_exit())
    print(nested_call_exit())
    print(nested_condition_exit())
    print(counted_init_exit())
    print(counted_step_exit())
    print(value_if_exit())
}
"#,
    )
    .unwrap();

    let mut bundle =
        jet::Loader::load_entry(file.to_str().unwrap()).expect("loop-value bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diag| matches!(diag.severity, jet::Diagnostics::Severity::Error))
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "loop-value bundle must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "loop values must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    match dev_iteration(file.to_str().unwrap(), false, true) {
        RunOutcome::Ran { stdout, .. } => {
            assert_eq!(
                stdout,
                "3\n[2, 4, 6, 8]\n9\n11\n12\n13\n14\n15\n16\n"
            )
        }
        other => panic!("loop values must run in the interpreter: {other:?}"),
    }
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|error| panic!("loop-value JIT compile failed: {error}"));

    jet_jit::reset_jit_trace_for_test();
    match dev_iteration(file.to_str().unwrap(), false, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert_eq!(
                stdout,
                "3\n[2, 4, 6, 8]\n9\n11\n12\n13\n14\n15\n16\n"
            )
        }
        other => panic!("loop values must run via native JIT: {other:?}"),
    }
    assert!(jet_jit::jit_executed_for_test(), "loop values must execute JIT");
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "loop values must not use interpreter fallback"
    );
}

#[test]
fn value_loop_named_routes_match_interpreter_default_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let dir = common::unique_tmp("jet_dev_value_loop_routes");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("value_loop_routes.jet");
    fs::write(
        &file,
        r#"fn run() {
    attempts := 0
    retry :: loop {
        attempts += 1
        found :: loop value, [1] {
            if attempts == 2 { break value }
        } ?? next(retry)
        print(found)
        break(retry)
    }
    stop :: loop {
        ignored :: loop value, [1] {
            if value == 2 { break value }
        } ?? break(stop)
        print(ignored)
    }
    print("done")
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().into_owned();
    let expected = ProgramOutput::ran("1\ndone\n".into(), String::new(), 0);

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("value-loop interpreter failed: {diags:?}"),
    };
    jet_jit::reset_jit_trace_for_test();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("value-loop default run failed: {diags:?}"),
    };
    assert!(jet_jit::jit_executed_for_test(), "value-loop routes must execute JIT");
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "value-loop routes must not use interpreter fallback"
    );
    let aot = compiled_binary_output(&dir, "value_loop_routes", 0, "value_loop_routes", &shown);

    assert_eq!(interpreted, expected, "interpreter value-loop route drift");
    assert_eq!(default, expected, "default JIT value-loop route drift");
    assert_eq!(aot, expected, "AOT value-loop route drift");
    let _ = fs::remove_dir_all(&dir);
}

/// E0956 regression: String field `+=` uses one owned concat operation on the
/// evaluator, resident JIT, and AOT paths.
#[test]
fn string_field_compound_append_matches_interpreter_default_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let dir = common::unique_tmp("jet_dev_string_field_compound");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("string_field_compound.jet");
    fs::write(
        &file,
        "struct Packet {\n    source: String\n    fn append(&self) {\n        self.source += \"AAA\"\n    }\n}\nfn run() {\n    p := Packet.{ source: \"base\" }\n    p.append()\n    print(p.source)\n}\n",
    )
    .unwrap();
    let shown = file.to_string_lossy().into_owned();
    let expected = ProgramOutput::ran("baseAAA\n".into(), String::new(), 0);

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("String field interpreter failed: {diags:?}"),
    };
    jet_jit::reset_jit_trace_for_test();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("String field default run failed: {diags:?}"),
    };
    assert!(
        jet_jit::jit_executed_for_test(),
        "String field compound append must execute in the resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "String field compound append must not raise E0956 through interpreter fallback"
    );
    let aot = compiled_binary_output(&dir, "string_field_compound", 0, "string_field_compound", &shown);

    assert_eq!(interpreted, expected, "String field interpreter route drift");
    assert_eq!(default, expected, "String field default JIT route drift");
    assert_eq!(aot, expected, "String field AOT route drift");
    let _ = fs::remove_dir_all(&dir);
}

/// c728 C6: one-shot `jet dev` deopts on a JIT gap and exits 0.
#[test]
fn one_shot_dev_deopts_on_jit_gap() {
    let dir = common::unique_tmp("jet_dev_one_shot_deopt");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("gap.jet");
    fs::write(
        &file,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", file.to_str().unwrap(), "--watch=off"])
        .env("NO_COLOR", "1")
        .output()
        .expect("one-shot jet dev");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("E2211"),
        "retired E2211 must not appear: {combined}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "deopted env.current_dir() should print a path: {combined}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// c728 C6: watching `jet dev` deopts on a gap edit and accepts a later valid edit.
#[test]
fn watching_dev_deopts_on_gap_edit_and_recovers() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    // Same shape as `watching_dev_reruns_on_jit_gap_and_recovers`: silent deopt
    // (no E2211), recover after gap edit. Do not require the watch banner —
    // WatchService timing is covered by UL6/native watch tests.
    let dir = std::env::temp_dir().join(format!("jet_watch_deopt_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("watch_gap.jet");
    fs::write(&file, "fn run() {\n    print(\"ok1\")\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", &shown])
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn watching jet dev");

    std::thread::sleep(Duration::from_millis(800));
    fs::write(
        &file,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    fs::write(&file, "fn run() {\n    print(\"ok2\")\n}\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));

    let _ = child.kill();
    let out = child.wait_with_output().expect("watching jet dev output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("ok1"), "stdout:\n{stdout}");
    assert!(
        !stderr.contains("E2211") && !stdout.contains("E2211"),
        "retired E2211 must not appear\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("ok2"), "stdout:\n{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

/// D-PERSIST1: the shipped teaching example shows mutation, compatible reload,
/// and shape-reset behavior through a real watching `jet dev` process.
#[test]
fn persist_example_survives_hot_reload() {
    use std::io::{BufRead, BufReader};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features/devloop/persist.jet");
    let dir = common::unique_tmp("jet_dev_persist_example");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("persist.jet");
    let original = fs::read(&source).unwrap();
    fs::write(&file, &original).unwrap();
    let shown = file.to_string_lossy().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", &shown, "--swap"])
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn persist example jet dev");
    let stdout = child.stdout.take().unwrap();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let wait_for = |expected: &str| {
        loop {
            let line = lines_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("persist example output")
                .expect("persist example stdout line");
            if line == expected {
                break;
            }
        }
    };

    wait_for("1");
    let compatible = String::from_utf8(original.clone())
        .unwrap()
        .replace("print(\"{counter}\")", "print(\"count={counter}\")");
    fs::write(&file, compatible).unwrap();
    wait_for("count=2");

    let shape_changed = String::from_utf8(original)
        .unwrap()
        .replace("#Persist counter := 0", "#Persist counter := 0.0")
        .replace("print(\"{counter}\")", "print(\"count={counter}\")");
    fs::write(&file, shape_changed).unwrap();
    wait_for("count=1.0");

    let _ = child.kill();
    let _ = child.wait_with_output().expect("stop persist example jet dev");
    let _ = fs::remove_dir_all(&dir);
}

/// D-AUTH-TOKENPOLICY1=A: auth verification stays resident in the default JIT;
/// forced interpretation uses the same ambient Prelude adapter.
#[test]
fn dev_default_runs_auth_verification_resident() {
    let dir = std::env::temp_dir().join(format!("jet_dev_auth_resident_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("auth_resident.jet");
    fs::write(
        &file,
        r#"use core.auth as auth

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    token := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrY"
    claims :: auth.verify_jwt(token, key: key, audience: "gateway") ?? panic("verification failed")
    print(claims.audience)
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();

    jet_jit::reset_jit_trace_for_test();
    match dev_iteration_with_timeout("auth_resident_interpreter", &shown, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stdout.trim(), "gateway");
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            assert!(!jet_jit::jit_executed_for_test());
            assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test());
        }
        RunOutcome::Problems(diags) => panic!("forced auth interpreter failed: {diags:?}"),
    }

    jet_jit::reset_jit_trace_for_test();
    match dev_iteration_with_timeout("auth_resident", &shown, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert!(
                jet_jit::jit_executed_for_test(),
                "default dev must execute core.auth in resident JIT"
            );
            assert!(!jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test());
            assert_eq!(stdout.trim(), "gateway");
        }
        RunOutcome::Problems(diags) => {
            panic!("default resident JIT failed: {diags:?}");
        }
    }

    let expected = compiled_binary_output(&dir, "auth_resident", 0, "auth_resident", &shown);
    assert_eq!(expected.stdout.trim(), "gateway");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SHAPE-PLACE1=A (#613): safe structural splitting follows the sema-proved
/// place identity and live range, not adjacent AST bindings. AOT and default
/// dev must preserve intervening effects and nested-field owner identity.
#[test]
fn place_split_planner_preserves_order_and_nested_owners() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping place split AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_place_split_planner_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("place_split.jet");
    fs::write(
        &file,
        r#"struct Holder { values: [Int] }
fn run() {
    values := [1, 2, 3, 4]
    first :: &values[0]
    print("between root")
    last :: &values[3]
    first = 8
    last = 9
    print("root: {first},{last}")

    adjacent := Holder.{ values: [10, 11, 12, 13] }
    adjacent_first :: &adjacent.values[0..1]
    adjacent_last :: &adjacent.values[2..3]
    adjacent_first[0] = 18
    adjacent_last[0] = 19
    print("nested adjacent: {adjacent_first[0]},{adjacent_last[0]}")

    interleaved := Holder.{ values: [20, 21, 22, 23] }
    interleaved_first :: &interleaved.values[0..1]
    print("between nested")
    interleaved_last :: &interleaved.values[2..3]
    interleaved_first[0] = 28
    interleaved_last[0] = 29
    print("nested interleaved: {interleaved_first[0]},{interleaved_last[0]}")

    reused := [30, 31]
    reused_first :: &reused[0]
    reused_bridge :: &reused[1]
    print("reuse first: {reused_first}")
    reused_again :: &reused[0]
    reused_bridge = 38
    reused_again = 39
    print("reuse final: {reused_again},{reused_bridge}")

    replaced := [40, 41]
    before_replace :: &replaced[0]
    print("before replace: {before_replace}")
    replaced = [42, 43]
    after_replace :: &replaced[1]
    #DebugOnly { print("unrelated debug") }
    after_replace = 49
    print("after replace: {after_replace}")
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let expected = compiled_binary_output(&dir, "place_split", 0, "place_split", &shown);
    assert_eq!(
        expected.stdout,
        "between root\nroot: 8,9\nnested adjacent: 18,19\nbetween nested\nnested interleaved: 28,29\nreuse first: 30\nreuse final: 39,38\nbefore replace: 40\nunrelated debug\nafter replace: 49\n"
    );
    assert_default_dev_jit_gap("place_split", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_parameter_view_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_parameter_view_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_parameter_view.jet");
    fs::write(
        &file,
        r#"fn first(left: [Int], right: [Int]) => View<Int> {
    return left[0..1]
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: first(left, right)
    print(result[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_parameter_view",
        0,
        "returned_parameter_view",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_parameter_view", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_view_field_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view-field AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_view_field_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_view_field.jet");
    fs::write(
        &file,
        r#"struct Window { values: View<Int> }

fn window(values: [Int]) => Window {
    selected :: values[0..1]
    return Window.{ values: selected }
}

fn run() {
    values := [7, 8]
    result :: window(values)
    print(result.values[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_view_field",
        0,
        "returned_view_field",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_view_field", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nested_returned_view_field_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping nested returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_nested_returned_view_field_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nested_returned_view_field.jet");
    fs::write(
        &file,
        r#"struct Inner { values: View<Int> }
struct Outer { inner: Inner }

fn outer(values: [Int]) => Outer {
    selected :: values[0..1]
    return Outer.{ inner: Inner.{ values: selected } }
}

fn run() {
    values := [7, 8]
    result :: outer(values)
    print(result.inner.values[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "nested_returned_view_field",
        0,
        "nested_returned_view_field",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("nested_returned_view_field", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn wrapped_returned_view_fields_match_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping wrapped returned-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_wrapped_returned_view_fields_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("wrapped_returned_view_fields.jet");
    fs::write(
        &file,
        r#"struct Window { values: View<Int> }
struct Holder { maybe: Window? }
struct GenericHolder<T> { value: T, maybe: Window? }
struct Node { next: Node?, values: View<Int> }

fn maybe(values: [Int]) => (Window?) {
    selected :: values[0..1]
    return Val(Window.{ values: selected })
}

fn result(values: [Int]) => Window ? String {
    selected :: values[0..1]
    return Ok(Window.{ values: selected })
}

fn tuple(values: [Int]) => (window: Window, count: Int) {
    selected :: values[0..1]
    return (window: Window.{ values: selected }, count: 1)
}

fn node(values: [Int]) => Node {
    selected :: values[0..1]
    return Node.{ next: None, values: selected }
}

fn run() { print(0) }
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "wrapped_returned_view_fields",
        0,
        "wrapped_returned_view_fields",
        &shown,
    );
    assert_eq!(aot.stdout, "0\n");
    assert_default_dev_jit_gap("wrapped_returned_view_fields", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_string_view_field_matches_all_execution_tiers() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-string-view AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_string_view_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_string_view.jet");
    fs::write(
        &file,
        r#"struct Parsed { source: String, head: View<str> }

fn parse(source: String) => Parsed {
    head :: source.before(":")
    return Parsed.{ source: source, head: head }
}

fn run() {
    left := "name"
    right := "value"
    source := "{left}:{right}"
    result :: parse(source)
    print(result.head)
    print("wrapped: {result.head}")
    expected :: "name"
    print("wrapped: {expected}")
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_string_view",
        0,
        "returned_string_view",
        &shown,
    );
    let expected = ProgramOutput::ran(
        "name\nwrapped: name\nwrapped: name\n".to_string(),
        String::new(),
        0,
    );
    assert_eq!(aot, expected, "AOT View<str> interpolation drifted from String");
    for (tier, use_interpreter) in [("default dev", false), ("interpreter", true)] {
        let output = match dev_iteration_with_timeout(
            "returned_string_view",
            &shown,
            use_interpreter,
        ) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("{tier} returned diagnostics for stored View<str>: {diags:?}")
            }
        };
        assert_eq!(output, expected, "{tier} View<str> interpolation drifted from String");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn returned_view_trait_method_matches_aot_and_default_dev() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping returned-view-trait AOT/dev regression");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_returned_view_trait_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("returned_view_trait.jet");
    fs::write(
        &file,
        r#"trait Select {
    fn select(self, left: [Int], right: [Int]) => View<Int>
}

struct First { marker: Int }
impl First.Select {
    fn select(self, left: [Int], right: [Int]) => View<Int> {
        return left[0..1]
    }
}

fn wrapper(selector: First, left: [Int], right: [Int]) => View<Int> {
    return selector.select(left, right)
}

fn run() {
    selector :: First.{ marker: 0 }
    left := [7, 8]
    right := [9, 10]
    result :: wrapper(selector, left, right)
    print(result[0])
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "returned_view_trait",
        0,
        "returned_view_trait",
        &shown,
    );
    assert_eq!(aot.stdout, "7\n");
    assert_default_dev_jit_gap("returned_view_trait", &shown);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn aggregate_trait_returns_match_aot_and_default_dev_in_both_impl_orders() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping aggregate trait-return AOT/dev regression");
        return;
    }
    let template = r#"struct Pair { left: View<Int>, right: View<Int> }
struct Envelope<T> { value: T, marker: Int }

trait Select {
    fn select(self, left: [Int], right: [Int]) => Pair
    fn optional(self, left: [Int], right: [Int]) => (Pair?)
    fn fallible(self, left: [Int], right: [Int]) => Pair ? String
    fn tupled(self, left: [Int], right: [Int]) => (pair: Pair, count: Int)
    fn generic(self, left: [Int], right: [Int]) => Envelope<Pair>
}

fn wrapper(selector: First, left: [Int], right: [Int]) => Pair {
    return selector.select(left, right)
}

$IMPLS

fn run() {
    left := [7, 8]
    right := [9, 10]
    pair :: wrapper(First.{ marker: 0 }, left, right)
    print(pair.left[0])
    print(pair.right[0])
}
"#;
    let implementation = |name: &str| {
        r#"struct $TYPE { marker: Int }
impl $TYPE.Select {
    fn select(self, left: [Int], right: [Int]) => Pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
    fn optional(self, left: [Int], right: [Int]) => (Pair?) {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Val(Pair.{ left: left_view, right: right_view })
    }
    fn fallible(self, left: [Int], right: [Int]) => Pair ? String {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Ok(Pair.{ left: left_view, right: right_view })
    }
    fn tupled(self, left: [Int], right: [Int]) => (pair: Pair, count: Int) {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return (pair: Pair.{ left: left_view, right: right_view }, count: 1)
    }
    fn generic(self, left: [Int], right: [Int]) => Envelope<Pair> {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Envelope<Pair>.{
            value: Pair.{ left: left_view, right: right_view },
            marker: 0,
        }
    }
}
"#
        .replace("$TYPE", name)
    };
    let first = implementation("First");
    let last = implementation("Last");
    for (index, implementations) in [
        format!("{first}{last}"),
        format!("{last}{first}"),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = std::env::temp_dir().join(format!(
            "jet_aggregate_trait_returns_{}_{}",
            std::process::id(),
            index
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aggregate_trait_returns.jet");
        fs::write(&file, template.replace("$IMPLS", &implementations)).unwrap();
        let shown = file.to_string_lossy().to_string();
        let stem = format!("aggregate_trait_returns_{index}");
        let aot = compiled_binary_output(&dir, &stem, 0, &stem, &shown);
        assert_eq!(aot.stdout, "7\n9\n");
        assert_default_dev_jit_gap(&stem, &shown);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn json_coerce_audit_reports_jit_gap_on_default_dev() {
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_json_coerce_fallback_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let stem = "serde/json_coerce";
    let file = example_path(stem);

    // `dev_iteration` returns `Problems` for a front-end rejection and for an
    // interpreter boundary alike, so the boundary assertion below cannot tell
    // "the interpreter refused to drop the audit effect" from "the fixture never
    // type-checked". Separate them here: this stem's whole point is the UNTYPED
    // D-JSON3 lenient `json.decode(text)` form, so if sema rejects that call the
    // failure is the stdlib surface, not the interpreter.
    let front_end: Vec<_> = jet::check_with_path(&file)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        front_end.is_empty(),
        "`{stem}` must type-check before the interpreter boundary means anything; \
         untyped `json.decode(text)` is the ratified D-JSON3 lenient form \
         (`module_items.rs` exports `decode`, `core_fixed_sig_impl` types it as \
         `Result<Data, JSONError>`, AOT lowers it to `jet_std_json_decode_lenient`): \
         {front_end:?}"
    );

    match dev_iteration_with_timeout(stem, &file, true) {
        RunOutcome::Problems(diags) => assert!(
            diags.iter().any(|d| d.code == "E2201"),
            "interpreter must name the coercion-audit boundary: {diags:?}"
        ),
        RunOutcome::Ran { .. } => {
            panic!("interpreter dropped the coercion audit effect instead of deferring to native")
        }
    }

    assert_default_dev_jit_gap(stem, &file);
    let expected = normalize_for_parity(
        stem,
        compiled_binary_output(&dir, "json_coerce_aot", 0, stem, &file),
    );
    assert_eq!(expected.stdout, "8081\napi\ntrue\n");
    assert_eq!(expected.exit_code, 0);
}

#[cfg(unix)]
#[test]
fn dev_default_socket_echo_reports_jit_gap() {
    let file = "examples/features/net/socket_echo.jet";
    assert_default_dev_jit_gap("net/socket_echo", file);
}

#[test]
fn dev_default_tls_peer_identity_matches_aot_and_interpreter() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let dir = std::env::temp_dir().join(format!(
        "jet_dev_tls_peer_identity_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let serial = dir.join("ca.srl");
    let server_cert = dir.join("server.cert.pem");
    let server_key = dir.join("server.key.pem");
    let csr = dir.join("server.csr.pem");
    let ext = dir.join("server.ext");
    fs::write(
        &ext,
        "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n",
    )
    .unwrap();
    let req = Command::new("openssl")
        .args([
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-keyout",
        ])
        .arg(&server_key)
        .arg("-out")
        .arg(&csr)
        .output()
        .unwrap();
    assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
    let sign = Command::new("openssl")
        .args([
            "x509",
            "-req",
            "-days",
            "1",
            "-CAcreateserial",
            "-CAserial",
        ])
        .arg(&serial)
        .arg("-CA")
        .arg(&ca_cert)
        .arg("-CAkey")
        .arg(&ca_key)
        .arg("-extfile")
        .arg(&ext)
        .arg("-in")
        .arg(&csr)
        .arg("-out")
        .arg(&server_cert)
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
    let root_bytes = fs::read(&ca_cert).unwrap();
    let jet_bytes = |bytes: &[u8]| {
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let source_for = |port: u16| {
        format!(
            r#"use core.net as net
use core.net.tls as tls

fn run() {{
    roots :: tls.RootCertificates.from_pem([U8].{{ {roots} }}) ?? panic("roots")
    cfg :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? panic("trust")
    cfg2 :: cfg.with_version_bounds(min: .Tls13, max: .Tls13) ?? panic("versions")
    tcp :: net.tcp_connect("127.0.0.1:{port}") ?? panic("tcp")
    budget :: Duration.seconds(2) ?? panic("deadline")
    secure := tls.client(^tcp, server_name: "localhost", config: cfg2, deadline: budget) ?? panic("tls")
    peer :: secure.peer_identity()
    print(peer.cipher_suite)
    print(peer.tls_version)
    if !peer.cipher_suite.starts_with("TLS13_") {{ panic("cipher") }}
    if peer.tls_version != .Tls13 {{ panic("version") }}
    secure.close() ?? panic("close")
}}
"#,
            roots = jet_bytes(&root_bytes),
        )
    };
    let wait_ready = |port: u16| {
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("openssl TLS 1.3 server did not accept on 127.0.0.1:{port}");
    };
    let start_server = |port: u16| {
        let server = Command::new("openssl")
            .args([
                "s_server",
                "-quiet",
                "-www",
                "-tls1_3",
                "-accept",
                &port.to_string(),
                "-cert",
            ])
            .arg(&server_cert)
            .arg("-key")
            .arg(&server_key)
            .arg("-cert_chain")
            .arg(&ca_cert)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("openssl TLS 1.3 server");
        wait_ready(port);
        server
    };
    let fresh_port = || {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        port
    };

    let aot_port = fresh_port();
    let file = dir.join("tls_peer_identity.jet");
    fs::write(&file, source_for(aot_port)).unwrap();
    let mut server = start_server(aot_port);
    let shown = file.to_string_lossy().to_string();
    let aot = compiled_binary_output(
        &dir,
        "tls_peer_identity_aot",
        0,
        "tls_peer_identity",
        &shown,
    );
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(aot.exit_code, 0, "AOT stderr: {}", aot.stderr);

    let dev_port = fresh_port();
    fs::write(&file, source_for(dev_port)).unwrap();
    let mut server = start_server(dev_port);
    let dev = match dev_iteration_with_timeout("tls_peer_identity", &shown, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev TLS peer identity failed: {diags:?}"),
    };
    let _ = server.kill();
    let _ = server.wait();

    let interpreter_port = fresh_port();
    fs::write(&file, source_for(interpreter_port)).unwrap();
    let mut server = start_server(interpreter_port);
    let interpreted = match dev_iteration_with_timeout("tls_peer_identity", &shown, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("interpreter TLS peer identity failed: {diags:?}"),
    };
    let _ = server.kill();
    let _ = server.wait();

    let mut aot_lines = aot.stdout.lines();
    assert!(aot_lines.next().is_some_and(|cipher| cipher.starts_with("TLS13_")));
    assert_eq!(aot_lines.next(), Some("Tls13"));
    assert_eq!(aot_lines.next(), None);
    assert_eq!(dev.stdout, aot.stdout);
    assert_eq!(interpreted.stdout, aot.stdout);
    assert_eq!(dev.exit_code, 0, "default dev stderr: {}", dev.stderr);
    assert_eq!(interpreted.exit_code, 0, "interpreter stderr: {}", interpreted.stderr);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dev_default_io_log_reports_jit_gap() {
    let file = "examples/features/io/log.jet";
    assert_default_dev_jit_gap("io/log", file);
}

#[test]
fn dev_default_resident_boundaries_report_jit_gap() {
    for stem in [
        "memory/entity_tree",
        "memory/expiring_secret",
    ] {
        let file = example_path(stem);
        assert_default_dev_jit_gap(stem, &file);
    }
}

/// c139 M4: scheduler/channel spawn stress example is resident-safe and runs.
#[test]
fn scheduler_spawn_runs_via_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/concurrency/scheduler_spawn.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("bundle should load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "scheduler_spawn must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "scheduler_spawn must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|e| panic!("scheduler_spawn JIT compile failed: {e}"));

    let got = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("scheduler_spawn must run via default dev/JIT, got: {ds:?}")
        }
    };
    assert_eq!(got.trim(), "1000");
}

#[test]
fn dev_default_interprets_display_debug_interpolation() {
    let file = "examples/features/types/display_debug.jet";
    // Named Display + JetDebug with #[Redact] now lower on the resident JIT.
    jet_jit::reset_jit_trace_for_test();
    match dev_iteration_with_timeout("types/display_debug", file, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert!(
                jet_jit::jit_executed_for_test(),
                "types/display_debug must run native JIT"
            );
            let gold = fs::read_to_string("examples/features/expected/types/display_debug.out")
                .expect("golden");
            assert_eq!(stdout, gold);
        }
        RunOutcome::Problems(diags) => {
            panic!("types/display_debug must run: {diags:?}");
        }
    }
}

#[test]
fn fixed_interpolation_matches_interpreter_and_resident_jit_rounding() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    lower_tie :: 1.125
    upper_tie :: 1.375
    grouped :: 1234.5678
    print("{lower_tie:Fixed(2)}|{upper_tie:Fixed(2)}|{grouped:Fixed(2)}")
}
"#;
    let expected = ProgramOutput::ran("1.12|1.38|1,234.57\n".into(), String::new(), 0);
    let file = std::env::temp_dir().join("jet_fixed_interpolation_parity.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("fixed interpolation must run in the interpreter: {diags:?}")
        }
    };

    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(src, "fixed_interpolation_parity");
    assert!(
        jet_jit::jit_executed_for_test(),
        "fixed interpolation must execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "fixed interpolation must not deopt or fall back"
    );
    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
}

#[test]
fn dev_packed_enum_print_is_safe_across_run_processes() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let cache = std::env::temp_dir().join(format!(
        "jet_dev_packed_enum_cache_{}_{}",
        std::process::id(),
        stamp
    ));
    let file = "examples/features/errors/errors.jet";
    let expected = "42\n84\nBadDigit(\"x\")\n";

    for run in 1..=2 {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", file])
            .env("JET_RUN_CACHE_DIR", &cache)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "run {run} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "run {run} must preserve the Jet enum name"
        );
    }

    let _ = fs::remove_dir_all(cache);
}

/// D-DEV1 "try anyway": the opt-in flag skips the boundary scan and attempts
/// execution. For a task program it then fails honestly at whatever
/// unsupported construct it actually hits during interpretation, rather than
/// refusing up front at the pre-scan's (earlier, more conservative) report
/// site — no guarantees, but it tried.
///
/// c139 JIT-parity fix (2026-07-03): the dev interpreter's own comptime-leak
/// errors (E0956/E3401) are now rewrapped as E2201 for a consistent voice
/// (`Source/Interpreter.rs::dev_boundary_from_comptime`), so the diagnostic
/// CODE alone no longer distinguishes "blocked by the pre-scan" from "tried
/// and failed later" — both surface as E2201. Compare the failure SITE
/// instead: try-anyway must fail at a different span than the pre-scan's
/// (earlier / more conservative) report, proving real execution proceeded
/// past the boundary before hitting trouble.
#[test]
fn try_anyway_skips_the_boundary_scan() {
    let path = std::env::temp_dir().join(format!(
        "jet_try_anyway_boundary_{}.jet",
        std::process::id()
    ));
    fs::write(
        &path,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let file = path.to_string_lossy().into_owned();
    let RunOutcome::Problems(blocked) = dev_iteration(&file, false, true) else {
        panic!("expected the E2201 pre-scan to block this program up front");
    };
    assert_eq!(blocked[0].code, "E2201", "pre-scan should report E2201");
    match dev_iteration(&file, true, true) {
        RunOutcome::Problems(diags) => {
            assert_ne!(
                diags.first().and_then(|d| d.span),
                blocked.first().and_then(|d| d.span),
                "try-anyway must fail at a different site than the pre-scan, proving it skipped the scan and actually tried"
            );
        }
        // If a future evaluator can run it, that's fine too — the point is the
        // pre-scan was skipped.
        RunOutcome::Ran { .. } => {}
    }
    let _ = fs::remove_file(path);
}

/// c139 M1: the Cranelift tier-1 backend runs `basics/hello.jet` with byte-identical
/// stdout to the interpreter baseline.
#[test]
fn cranelift_backend_matches_hello() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/basics/hello.jet";
    // Load and check on the canonical compiler worker: doing it inline here is
    // what aborted the whole dev binary, since a 2 MiB libtest worker cannot
    // hold the loader and sema recursion. `checked_bundle_from_path` also
    // asserts the fixture type-checks, which is what the inline block did.
    let bundle = checked_bundle_from_path(file);

    let expected = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => {
            panic!("interpreter baseline must run hello, got diagnostics: {ds:?}")
        }
    };

    let mut backend = CraneliftBackend::new();
    let got = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("cranelift backend did not run hello: {ds:?}"),
    };
    assert_eq!(got, expected, "cranelift output drifted from interpreter");

    let src = fs::read_to_string(file).expect("hello source should read");
    let compiled = jet::compile_with_path(&src, file).expect("hello should compile");
    let rs = std::env::temp_dir().join("jet_dev_cranelift_hello.rs");
    let bin = std::env::temp_dir().join("jet_dev_cranelift_hello");
    let mut command = Command::new("rustc");
    add_generated_rust(
        &mut command,
        &rs,
        &compiled.rust,
        compiled.ffi.is_some(),
        &[],
    );
    let rustc = command.arg("-o").arg(&bin).output().expect("run rustc");
    assert!(
        rustc.status.success(),
        "rustc failed compiling hello fixture: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().expect("run compiled hello");
    let compiled_stdout = String::from_utf8_lossy(&run.stdout).to_string();
    assert_eq!(
        got, compiled_stdout,
        "cranelift output drifted from AOT binary"
    );
}

#[test]
fn post_contract_failure_matches_aot_under_quick_run() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
#Post(result == 99, "must equal 99")
fn get() => Int {
    return 1
}

fn run() {
    print(get())
}
"#;
    let dir = std::env::temp_dir().join(format!(
        "jet_contract_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("post_failure.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();

    let quick = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(ds) => panic!("quick run returned diagnostics: {ds:?}"),
    };
    let aot = compiled_binary_output(&dir, "post_contract", 0, "contracts/post_failure", &shown);

    assert_eq!(quick.exit_code, 70, "quick run must trap: {quick:?}");
    assert!(quick.stderr.contains("#Post contract failed: must equal 99"));
    assert_eq!(quick, aot, "quick run and AOT contract behavior diverged");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unicode_16_string_and_core_text_match_aot_comptime_and_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let upper_only_in_host_unicode_17 = char::from_u32(0xA7CE).unwrap();
    let lower_only_in_host_unicode_17 = char::from_u32(0xA7CF).unwrap();
    let whitespace = char::from_u32(0x2003).unwrap();
    let source = format!(
        r#"use core.text as text

fn run() {{
    print("{upper_only_in_host_unicode_17}".to_lower() == "{upper_only_in_host_unicode_17}")
    print("{lower_only_in_host_unicode_17}".to_upper() == "{lower_only_in_host_unicode_17}")
    print("{whitespace}jet{whitespace}".trim())
    print(text.lower("{upper_only_in_host_unicode_17}") == "{upper_only_in_host_unicode_17}")
    print(text.upper("{lower_only_in_host_unicode_17}") == "{lower_only_in_host_unicode_17}")
    print(text.trim("{whitespace}jet{whitespace}"))
}}
"#,
    );
    let expected = "true\ntrue\njet\ntrue\ntrue\njet\n";

    let resident = run_cranelift_without_fallback(&source, "unicode_16_public_string");
    assert_eq!(resident.stdout, expected);

    let dir = common::unique_tmp("jet_unicode_16_public_string_aot");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("main.jet");
    let rust_path = dir.join("main.rs");
    let binary = dir.join("main");
    fs::write(&jet_path, &source).unwrap();
    let compiled = jet::compile_with_path(&source, &jet_path.to_string_lossy())
        .expect("Unicode-16 String fixture should compile");
    let mut command = Command::new("rustc");
    add_generated_rust(
        &mut command,
        &rust_path,
        &compiled.rust,
        compiled.ffi.is_some(),
        &[],
    );
    let rustc = command.arg("-o").arg(&binary).output().unwrap();
    assert!(
        rustc.status.success(),
        "Unicode-16 AOT fixture rejected:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let aot = Command::new(&binary).output().unwrap();
    assert!(aot.status.success());
    assert_eq!(String::from_utf8_lossy(&aot.stdout), expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unsupported_core_text_is_not_claimed_by_resident_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jet_unicode_16_jit_boundary");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(
        &path,
        "use core.text as text\nfn run() { print(text.casefold(\"Straße\")) }\n",
    )
    .unwrap();
    let bundle = checked_bundle_from_path(&path.to_string_lossy());
    assert!(!jet_jit::resident_jit_safe_bundle(&bundle));
    assert!(jet_jit::resident_jit_safe_bundle_detail(&bundle).contains("entry not resident-safe"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn exact_int_equality_matches_aot_in_resident_and_default_dev() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    left :: -999999999999999999999999999999
    same :: -999999999999999999999999999999
    other :: 999999999999999999999999999999
    print(left == same)
    print(left != same)
    print(left != other)
}
"#;
    let resident = run_cranelift_without_fallback(src, "exact_int_value_equality");

    let dir = std::env::temp_dir().join(format!("jet_exact_int_equality_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("exact_int_value_equality.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed exact Int equality: {diags:?}"),
    };
    let aot = compiled_binary_output(
        &dir,
        "exact_int_value_equality",
        0,
        "exact_int_value_equality",
        &shown,
    );
    let expected = ProgramOutput::ran("true\nfalse\ntrue\n".to_string(), String::new(), 0);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn default_err_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/errors/default_err_edge.jet";
    let expected_stderr = fs::read_to_string(
        "examples/features/expected/errors/default_err_edge.err.out",
    )
    .expect("default_err_edge.err.out");
    let expected = ProgramOutput::ran(String::new(), expected_stderr, 1);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => {
            panic!("default Err example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(&source, "default_err");
    assert!(jet_jit::jit_executed_for_test(), "default Err must execute as native JIT");
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "default Err must not use interpreter deopt or fallback"
    );
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed default Err: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_default_err_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "default_err", 0, "default_err", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exact_int_example_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/text/int_exact.jet";
    let expected = ProgramOutput::ran(golden_stdout("text/int_exact"), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => {
            panic!("exact Int example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(&source, "exact_int_example");
    assert!(
        jet_jit::jit_executed_for_test(),
        "exact Int example must execute as native JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "exact Int example must not use interpreter deopt or fallback"
    );
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed exact Int example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_exact_int_example_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "exact_int_example", 0, "exact_int_example", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn type_alias_example_matches_golden_on_all_execution_tiers() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/types/type_alias.jet";
    let expected = ProgramOutput::ran(golden_stdout("types/type_alias"), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => {
            panic!("type_alias example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    let resident = run_cranelift_resident(&source, "type_alias_golden");
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed type_alias example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_type_alias_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "type_alias_golden", 0, "types/type_alias", file);

    assert_eq!(interpreted, expected, "type_alias interpreter output drifted");
    assert_eq!(resident, expected, "type_alias resident JIT output drifted");
    assert_eq!(default, expected, "type_alias default dev output drifted");
    assert_eq!(aot, expected, "type_alias AOT output drifted");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn archive_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/io/archive.jet";
    let expected = ProgramOutput::ran(golden_stdout("io/archive"), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("Archive example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(&source, "archive");
    assert!(
        jet_jit::jit_executed_for_test(),
        "Archive example must execute as native JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "Archive example must not use interpreter deopt or fallback"
    );
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed Archive example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_archive_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "archive", 0, "archive", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn progress_reporter_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/io/progress.jet";
    let expected = ProgramOutput::ran(golden_stdout("io/progress"), String::new(), 0);

    let interpreted = match dev_iteration_with_timeout("progress_reporter", file, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("progress example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident_source = source.clone();
    let (resident, resident_flags, resident_trace) = with_jit_test_scope(move || {
        jet_jit::set_trace_tiers(true);
        let resident = run_cranelift_without_fallback(&resident_source, "progress_reporter");
        let flags = jet_jit::jit_trace_flags_for_test();
        let trace = jet_jit::take_last_trace();
        (resident, flags, trace)
    });
    jet_jit::merge_jit_trace_flags_for_test(resident_flags);
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "progress example must not fall back to a second runtime"
    );
    // A resident JIT may deopt an unsupported adapter. I9 requires the same
    // Prelude meaning after deopt; it does not require every collection shape
    // to have a native Cranelift lowering.
    let _native_jit = jet_jit::jit_executed_for_test();
    let _trace = resident_trace;
    let default = match dev_iteration_with_timeout("progress_reporter", file, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed progress example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_progress_reporter_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "progress_reporter", 0, "progress_reporter", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn set_union_matches_interpreter_resident_jit_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let file = "examples/features/collections/set.jet";
    let expected = ProgramOutput::ran(golden_stdout("collections/set"), String::new(), 0);

    let interpreted = match dev_iteration(file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("Set example must execute in interpreter tier: {diags:?}")
        }
    };

    let source = fs::read_to_string(file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_resident(&source, "set_union");
    assert!(
        jet_jit::jit_executed_for_test(),
        "Set example must execute as native JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "Set example must not use interpreter deopt or fallback"
    );
    let default = match dev_iteration(file, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed Set example: {diags:?}"),
    };

    let dir = std::env::temp_dir().join(format!("jet_set_union_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "set_union", 0, "set_union", file);

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unified_loop_jit_tiers_are_explicit_and_match_aot() {
    let counted = "fn run() {\n    loop i, 0..<4 {\n        if i == 1 { next }\n        print(i)\n    }\n}\n";
    if !skip_if_cranelift_host_unsupported() {
        let native = run_cranelift_without_fallback(counted, "counted_next");
        assert_eq!(native.stdout, "0\n2\n3\n");
    }

    let stride = "fn run() {\n    xs := [0, 1, 2, 3, 4]\n    loop x, xs, 2 {\n        print(x)\n        if x == 0 { next }\n    }\n}\n";
    if !skip_if_cranelift_host_unsupported() {
        let native = run_cranelift_without_fallback(stride, "source_stride_next");
        assert_eq!(native.stdout, "0\n2\n4\n");

        let invalid = "fn run() {\n    xs := [1, 2]\n    stride := 0\n    loop x, xs, stride {\n        print(x)\n    }\n}\n";
        // AOT emits jet_panic("E0123: …") → exit 70 + panic: wording (I9).
        // Resident JIT must match that shape, not a Problems diagnostic.
        match run_cranelift_outcome_without_fallback(invalid, "source_stride_pre_pull") {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 70, "invalid stride must exit 70: out={stdout} err={stderr}");
                assert!(
                    stdout.is_empty(),
                    "invalid dynamic stride must stop before the first source pull, got stdout={stdout:?}"
                );
                assert!(
                    stderr.contains("panic:") && stderr.contains("E0123"),
                    "invalid stride must carry E0123 in panic wording, got: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "invalid stride must not use comptime voice: {stderr}"
                );
            }
            other => panic!("invalid dynamic stride expected runtime trap, got: {other:?}"),
        }
    }
}

#[test]
fn range_values_run_in_resident_jit_without_fallback() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let unboxed = r#"
fn identity(band: ^Range) => Range {
    return band
}
fn run() {
    band := 2..<5
    copied :: identity(band)
    print(copied)
    print(band.start)
    print(copied.contains(4))
    print(copied == band)
    bands :: [1..3, 8..<10]
    print("{bands[1]:Debug}")
    print(bands[0].start)
    print(bands[0].contains(3))
    total := 0
    loop n, copied {
        total += n
    }
    print(total)
    values :: [10, 20, 30, 40, 50, 60]
    print(~values[copied])
    band = 7..9
    print(band)
}
"#;
    // The unboxed block used to prove only stdout. `run_cranelift_without_fallback`
    // accepts a silent deopt to the interpreter, so the test's own name -- "run in
    // resident jit without fallback" -- was never checked for this fixture. Prove
    // the resident tier first, the same way the `src` block below does.
    //
    // `try_compile_bundle` comes first on purpose: `resident_jit_func_safety_detail`
    // returns `None` (the "covered" answer) when `lower_jit_program` yields nothing,
    // so on a lowering failure it reads green. The compile hook reports the real
    // reason instead.
    let unboxed_proof = std::env::temp_dir().join("jet_jit_range_unboxed_safety.jet");
    fs::write(&unboxed_proof, unboxed).unwrap();
    let unboxed_bundle = checked_bundle_from_path(&unboxed_proof.to_string_lossy());
    jet_jit::try_compile_bundle(&unboxed_bundle)
        .unwrap_or_else(|error| panic!("unboxed Range resident compilation failed: {error}"));
    assert_eq!(
        jet_jit::resident_jit_func_safety_detail(&unboxed_bundle, "run"),
        jet_jit::ResidentJitSafety::Covered,
        "unboxed Range values must stay in resident JIT"
    );
    jet_jit::reset_struct_new_count_for_test();
    let unboxed_run = run_cranelift_without_fallback(unboxed, "range_unboxed");
    assert_eq!(
        unboxed_run.stdout,
        "Range { start: 2, end: 5, exclusive: true }\n2\ntrue\ntrue\nRange { start: 8, end: 10, exclusive: true }\n1\ntrue\n9\n[30, 40, 50]\nRange { start: 7, end: 9, exclusive: false }\n"
    );
    assert_eq!(
        jet_jit::struct_new_count_for_test(),
        0,
        "Range construct/copy/pass/return/list/field/contains/show/equality/loop/slice must not call struct_new"
    );

    let src = r#"
fn identity(band: ^Range) => Range {
    return band
}
fn run() {
    band :: 2..<5
    copied :: identity(band)
    print(copied == band)
    bands :: [1..3, 8..<10]
    print(bands[0])
    print("{bands[1]:Debug}")
    print(bands[0].contains(3))
    print(band)
    print("{band}")
    print("{band:Debug}")
    print(band == (2..<5))
    print(band == (2..5))
    print(band.start)
    print(band.end)
    print(band.contains(4))
    print((5..2).contains(3))
    total := 0
    loop n, band {
        total += n
    }
    print(total)
    values := [10, 20, 30, 40, 50, 60]
    print(~values[band])
    edit :: &values[band]
    edit[0] = 99
    print(values)
}
"#;
    let proof = std::env::temp_dir().join("jet_jit_range_value_safety.jet");
    fs::write(&proof, src).unwrap();
    let bundle = checked_bundle_from_path(&proof.to_string_lossy());
    // Same order as the unboxed block above, for the same reason:
    // `resident_jit_func_safety_detail` answers `None` ("covered") whenever
    // `lower_jit_program` yields nothing, so on a lowering failure it reads
    // green. Compile first so this half also names the real reason.
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|error| panic!("Range resident compilation failed: {error}"));
    assert_eq!(
        jet_jit::resident_jit_func_safety_detail(&bundle, "run"),
        jet_jit::ResidentJitSafety::Covered,
        "Range values and windows must stay in resident JIT"
    );
    let native = run_cranelift_without_fallback(src, "range_values");
    let expected = "\
true
Range { start: 1, end: 3, exclusive: false }
Range { start: 8, end: 10, exclusive: true }
true
Range { start: 2, end: 5, exclusive: true }
Range { start: 2, end: 5, exclusive: true }
Range { start: 2, end: 5, exclusive: true }
true
false
2
5
true
false
9
[30, 40, 50]
[10, 20, 99, 40, 50, 60]
";
    assert_eq!(native.stdout, expected);

    let dir = common::unique_tmp("jet_range_value_interpreter");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    match dev_iteration(&path.to_string_lossy(), false, true) {
        RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, expected),
        RunOutcome::Problems(diags) => {
            panic!("Range views must run in the canonical evaluator: {diags:?}")
        }
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn subjectless_guards_match_aot_in_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    n :: 7
    if n > 10 -> print("too big")
    if {
        n < 0 -> print("negative")
        n < 10 -> print("single digit")
    }
    label :: if {
        n < 5 -> "small"
        n < 10 -> "medium"
        else -> "large"
    }
    print(label)
}
"#;
    let jit = run_cranelift_without_fallback(src, "subjectless_guards");
    let dir = std::env::temp_dir().join(format!("jet_guard_jit_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("subjectless_guards.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(
        &dir,
        "subjectless_guards",
        0,
        "subjectless_guards",
        file.to_str().unwrap(),
    );
    assert_eq!(jit, aot);
    assert_eq!(jit.stdout, "single digit\nmedium\n");
}

#[test]
fn resident_jit_numeric_methods_and_parse_are_native() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    Int.parse("41").drop("native parse proof")
    n :: 41
    print(Float.from_int(n))
    print(n.count_ones())
    print(1.0.is_finite())
    print(n.to_string())
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "numeric_parse"),
        ProgramOutput::ran("41.0\n3\ntrue\n41\n".into(), "".into(), 0)
    );
}

#[test]
fn forced_interpreter_preserves_f32_width_like_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping F32 dev differential");
        return;
    }
    let source = r#"fn pass(value: F32) => F32 { return value }
fn run() {
    value :: F32.{ 16777217.0 }
    one :: F32.{ 1.0 }
    mutable := F32.{ value }
    mutable += one
    print(pass(value))
    print(mutable)
    print([value, mutable])
}"#;
    let dir = common::unique_tmp("jet_dev_f32_width");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f32_width.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "f32_width", 0, "f32-width", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced F32 interpreter failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(
        actual.stdout,
        "16777216.0\n16777216.0\n[16777216.0, 16777216.0]\n"
    );
}

#[test]
fn gzip_golden_matches_forced_interpreter_and_aot() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping gzip dev differential");
        return;
    }
    let source = r#"use core.archive.gzip as gzip

fn run() {
    bytes :: [U8].{ 72, 101, 108, 108, 111 }
    gz :: gzip.decompress(gzip.compress(bytes)) ?? [U8].{}
    golden :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 5, 0, 0, 0]) ?? [U8].{}
    bad_size :: gzip.decompress([31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 203, 72, 205, 201, 201, 7, 0, 134, 166, 16, 54, 6, 0, 0, 0]) ?? [U8].{ 255 }
    h :: U8.{ 72 }
    lower_h :: U8.{ 104 }
    o :: U8.{ 111 }
    max :: U8.{ 255 }
    print(gz.len() == 5)
    print(gz[0] == h)
    print(golden.len() == 5)
    print(golden[0] == lower_h)
    print(golden[4] == o)
    print(bad_size[0] == max)
}
"#;
    let dir = common::unique_tmp("jet_dev_compression");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("compression.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "compression", 0, "compression", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("forced compression interpreter failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn zstd_compress_runs_in_forced_interpreter_with_aot_wire_shape() {
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping zstd dev differential");
        return;
    }
    let source = r#"use core.archive.zstd as zstd

fn run() {
    frame :: zstd.compress([72, 101, 108, 108, 111])
    m0 :: U8.{ 40 }
    m1 :: U8.{ 181 }
    m2 :: U8.{ 47 }
    m3 :: U8.{ 253 }
    print(frame.len() > 9)
    print(frame[0] == m0 && frame[1] == m1 && frame[2] == m2 && frame[3] == m3)
}
"#;
    let dir = common::unique_tmp("jet_dev_zstd_compress");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zstd_compress.jet");
    fs::write(&path, source).unwrap();
    let file = path.to_string_lossy();
    let expected = compiled_binary_output(&dir, "zstd_compress", 0, "zstd-compress", &file);
    let actual = match dev_iteration(&file, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("forced zstd compressor failed: {diags:?}"),
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resident_jit_checked_numeric_and_distinct_conversion_matrix_is_native() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
#Numeric UserId :: distinct Int
#Numeric Severity :: distinct Int(0..10)
#UnitFamily(Currency) { usd }

fn run() {
    print(I64.from_u8(255))
    byte_ok :: I32.{ 100 }
    byte_bad :: I32.{ 100000 }
    U8.from_i32(byte_ok).drop("checked conversion success proof")
    U8.from_i32(byte_bad).drop("checked conversion error proof")
    float_ok :: 42.9
    float_bad :: 300.0
    U8.from_float(float_ok).drop("checked float conversion success proof")
    U8.from_float(float_bad).drop("checked float conversion error proof")
    narrow_ok :: 2.5
    narrow_bad :: 1e100
    F32.from_float(narrow_ok).drop("checked F32 conversion success proof")
    F32.from_float(narrow_bad).drop("checked F32 conversion error proof")
    user_source :: U64.from_u8(8)
    UserId.from_u64(user_source).drop("checked distinct conversion proof")
    print(UserId.from_u8(8).raw())
    print(Severity.from_u8(8).raw())
    severity_source :: 7
    Severity.from_int(severity_source).drop("checked range conversion proof")
    print(Severity.from_int(7).raw())
    print(Usd.from_int(5).raw())
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "checked_numeric_distinct_matrix"),
        ProgramOutput::ran(
            "255\n8\n8\n7\n5.0\n".into(),
            "".into(),
            0,
        )
    );
}

#[test]
fn physical_quantities_run_in_resident_jit_without_fallback() {
    if skip_if_cranelift_host_unsupported() { return; }
    let out = run_cranelift_without_fallback(r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
    thirdish(scale: 2/3)
}
fn run() ? {
    distance :: 12meter
    elapsed :: 3s
    speed :: distance / elapsed
    recovered :: speed * elapsed
    ratio :: recovered / distance
    print(ratio)
    exact :: Meter.from_millimeter(3000millimeter)?
    rounded :: Meter.from_thirdish_rounded(1thirdish, .NearestEven, digits: 0)?
    print("{(exact.raw())} {(rounded.raw())}")
}
"#, "physical_quantity");
    assert_eq!(out.stdout, "1.0\n3.0 1.0\n");

    // `run_cranelift_without_fallback` writes each fixture at this exact path,
    // and every E3002 journey frame names it, so the expectations below must be
    // built from it rather than from a hand-written literal.
    let fixture_shown = |tag: &str| {
        std::env::temp_dir()
            .join(format!("jet_jit_no_fallback_{tag}.jet"))
            .to_string_lossy()
            .into_owned()
    };

    let failed = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    thirdish(scale: 2/3)
}
fn run() ? {
    Meter.from_thirdish(1thirdish)?
}
"#, "physical_quantity_inexact");
    // D-FAIL-CTX1=A (ratified 2026-08-06, card #1532): "Each `?` hop joins the
    // failure journey on every tier, whether it has a note or not." The `?` on
    // line 7 is that hop, printed as one trail line under the root failure the
    // way E3002 now registers it: `  {n}. {fn} ({file}:{line})`, origin first,
    // under the root failure line. D-FAIL-ERROR1=A
    // (card #1528) + D-FAIL-EXIT1=A (card #1533): bare `fn run() ?` means
    // `run() ? Err`, so the conversion's plain `String` error arrives as the
    // default error and `jet_render_err` prints `Error: {message}` -- no code,
    // because this error carries none -- at the process edge, then exits 1.
    let inexact_shown = fixture_shown("physical_quantity_inexact");
    assert_eq!(
        failed,
        ProgramOutput::ran(
            String::new(),
            format!(
                "Error: unit conversion would round\n\
                 \x20Trail [E3002] (1 hop via ?, origin first):\n\
                 \x20 1. run ({inexact_shown}:7)\n"
            ),
            1
        )
    );

    let beyond_f64 = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
}
fn run() ? {
    Meter.from_almost(1almost)?
}
"#, "physical_quantity_exact_rational_edge");
    // Same ratified envelope as above (D-FAIL-CTX1=A / D-FAIL-ERROR1=A): the
    // exact conversion's `?` is again on line 7 of the fixture.
    let rational_edge_shown = fixture_shown("physical_quantity_exact_rational_edge");
    assert_eq!(
        beyond_f64,
        ProgramOutput::ran(
            String::new(),
            format!(
                "Error: unit conversion would round\n\
                 \x20Trail [E3002] (1 hop via ?, origin first):\n\
                 \x20 1. run ({rational_edge_shown}:7)\n"
            ),
            1
        )
    );

    let rational_edges = run_cranelift_without_fallback(r#"
#UnitFamily(Length, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
    half(scale: 1/2)
    above_half(scale: 9007199254740993/18014398509481984)
    three_halves(scale: 3/2)
}
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    tie_offset(scale: 1, offset: 1/2)
    above_offset(scale: 1, offset: 9007199254740993/18014398509481984)
    below_offset(scale: 1, offset: -9007199254740993/18014398509481984)
}
fn run() ? {
    tie :: Meter.from_half_rounded(1half, .NearestEven, digits: 0)?
    above :: Meter.from_above_half_rounded(1above_half, .NearestEven, digits: 0)?
    negative_source :: ThreeHalves.from_float(-1.0)
    negative :: Meter.from_three_halves_rounded(negative_source, .NearestEven, digits: 0)?
    tie_point :: TieOffsetPoint.from_float(0.0)
    above_point :: AboveOffsetPoint.from_float(0.0)
    below_point :: BelowOffsetPoint.from_float(0.0)
    affine_tie :: KelvinPoint.from_tie_offset_point_rounded(tie_point, .NearestEven, digits: 0)?
    affine_above :: KelvinPoint.from_above_offset_point_rounded(above_point, .NearestEven, digits: 0)?
    affine_below :: KelvinPoint.from_below_offset_point_rounded(below_point, .NearestEven, digits: 0)?
    print("{(tie.raw())} {(above.raw())} {(negative.raw())} {(affine_tie.raw())} {(affine_above.raw())} {(affine_below.raw())}")
}
"#, "physical_quantity_rational_edges");
    assert_eq!(
        rational_edges,
        ProgramOutput::ran("0.0 1.0 -2.0 0.0 1.0 -1.0\n".into(), "".into(), 0)
    );

    let overflow = r#"
#UnitFamily(Length, base: meter) { meter double(scale: 2) }
fn run() ? {
    source :: Double.from_float(1.7976931348623157e308)
    Meter.from_double_rounded(source, .NearestEven, digits: 0)?
}
"#;
    // Same ratified envelope (D-FAIL-CTX1=A / D-FAIL-ERROR1=A); this fixture's
    // `?` is on line 5, so its single trail hop names that line.
    let overflow_shown = fixture_shown("physical_quantity_rounded_overflow");
    assert_eq!(
        run_cranelift_without_fallback(overflow, "physical_quantity_rounded_overflow"),
        ProgramOutput::ran(
            String::new(),
            format!(
                "Error: unit conversion overflows its runtime representation\n\
                 \x20Trail [E3002] (1 hop via ?, origin first):\n\
                 \x20 1. run ({overflow_shown}:5)\n"
            ),
            1
        )
    );
}

#[test]
fn rounded_physical_quantities_match_resident_default_dev_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
#UnitFamily(Length, base: meter) {
    meter
    half(scale: 1/2)
    near_quarter(scale: 249/1000)
    near_three_quarters(scale: 751/1000)
}
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    shifted(scale: 1, offset: 249/1000)
}
fn run() ? {
    positive :: Half.from_float(5.0)
    negative :: Half.from_float(-5.0)
    toward_zero :: Meter.from_half_rounded(positive, .TowardZero, digits: 0)?
    floor :: Meter.from_half_rounded(negative, .Floor, digits: 0)?
    ceiling :: Meter.from_half_rounded(positive, .Ceiling, digits: 0)?
    nearest_even :: Meter.from_near_quarter_rounded(1near_quarter, .NearestEven, digits: 2)?
    nearest_odd :: Meter.from_near_three_quarters_rounded(1near_three_quarters, .NearestEven, digits: 2)?
    point :: KelvinPoint.from_shifted_point_rounded(ShiftedPoint.from_float(0.0), .Ceiling, digits: 2)?
    delta :: KelvinDelta.from_shifted_delta_rounded(ShiftedDelta.from_float(0.0), .Ceiling, digits: 2)?
    print("{(toward_zero.raw())} {(floor.raw())} {(ceiling.raw())} {(nearest_even.raw())} {(nearest_odd.raw())} {(point.raw())} {(delta.raw())}")
}
"#;
    let expected = ProgramOutput::ran("2.0 -3.0 3.0 0.25 0.75 0.25 0.0\n".into(), "".into(), 0);
    let resident = run_cranelift_without_fallback(src, "rounded_quantity_parity");

    let dir = std::env::temp_dir().join(format!(
        "jet_rounded_quantity_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rounded_quantity_parity.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(diags) => panic!("default dev failed rounded quantity parity: {diags:?}"),
    };
    let aot = compiled_binary_output(&dir, "rounded_quantity_parity", 0, "rounded_quantity_parity", &shown);
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn generic_module_instance_runs_identically_in_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() { return; }
    let src = r#"
module value(n: Int) { pub fn get() => Int { return n } }
module three :: value(3)
module same :: value(3)
fn run() { print(three.get()); print(same.get()) }
"#;
    let jit = run_cranelift_without_fallback(src, "generic_module_instance");
    assert_eq!(jit.stdout, "3\n3\n");

    let dir = std::env::temp_dir().join(format!("jet_generic_module_jit_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_module_instance.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(&dir, "generic_module_instance", 0, "generic_module_instance", file.to_str().unwrap());
    assert_eq!(jit, aot);

    let bundle = checked_bundle_from_path(file.to_str().unwrap());
    let tir = jet::Codegen::TIR::lower_jit_program(&bundle).expect("generic instance lowers to JIT TIR");
    assert_eq!(tir.instance_provenance.len(), 1, "equivalent aliases share one canonical instance");
    // architecture.md R6 / ratified D-NAME-TREE1 (docs/spec/architecture.md:554-559):
    // "User identifiers are emitted as `__jet_<name>` … all Rust-name projections
    // use its canonical mangle functions." An inline-module member goes through
    // `Names::member_name` (crates/jet-codegen/src/Codegen/TIR/mod.rs:1679,1683),
    // i.e. `generated_path("three.get")` → `__jet_three__get`; AOT emits the same
    // symbol from `Codegen/Imports.rs::emit_program_items`. Ask the naming law for
    // the name rather than restating a spelling — 8044b2e69 (#1801) already moved
    // it once from the old `{module}__{fn}` form and left this literal behind.
    let member = jet_foundation::Names::member_name("three", "get");
    assert_eq!(tir.funcs.iter().filter(|f| f.name == member).count(), 1);
}

#[test]
fn generic_user_derive_multi_instantiation_matches_every_execution_tier() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let src = r#"
derive T.Access {
    info :: T.reflect()
    param :: info.type_params[0].name
    fn make(value: ^@param) => @name<@param> {
        return @name<@param>.{ value: value }
    }
    fn marker() => Int :: 17
    fn get_value(self) => @param :: ~self.value
    fn type_name(self) => String :: T.@name
}

derive T.NumericAccess {
    info :: T.reflect()
    param :: info.type_params[0].name
    fn replace(&self, value: ^@param) => @param {
        self.value = value
        return ~self.value
    }
    fn plus(self, rhs: @param) => @param :: self.value + rhs
    fn equal_to(self, rhs: @param) => Bool :: self.value == rhs
}

#Access
struct Box<T: Printable> { value: T }
#Access
struct StaticOnly<T: Printable> { value: T }
struct Wrapper<U: Printable> { boxed: Box<U> }
#NumericAccess
struct NumericBox<T: [Printable, Add, Equatable]> { value: T }

fn run() {
    number := Box<Int>.make(7)
    decimal := Box<Float>.{ value: 2.5 }
    flag := Box<Bool>.{ value: true }
    letter := Box<Char>.{ value: 'J' }
    text := Box<String>.make("jet")
    numeric := NumericBox<Float>.{ value: 1.5 }
    print(number.get_value())
    print(decimal.get_value())
    print(flag.get_value())
    print(letter.get_value())
    print(text.get_value())
    print(text.get_value())
    print(number.type_name())
    print(numeric.replace(4.5))
    print(numeric.plus(0.5))
    print(numeric.equal_to(4.5))
    print(StaticOnly<Int>.marker())
    print(StaticOnly<String>.marker())
}
"#;
    let expected = ProgramOutput::ran(
        "7\n2.5\ntrue\nJ\njet\njet\nBox\n4.5\n5.0\ntrue\n17\n17\n".into(),
        "".into(),
        0,
    );
    let dir = std::env::temp_dir().join(format!(
        "jet_generic_user_derive_jit_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_user_derive.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    let run_func = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(func) if func.name == "run" => Some(func),
            _ => None,
        })
        .expect("run function");
    let numeric_binding = run_func
        .body
        .iter()
        .find_map(|stmt| match stmt {
            jet::AST::Stmt::Val(binding) if binding.name == "numeric" => Some(binding),
            _ => None,
        })
        .expect("numeric binding");
    assert_eq!(
        numeric_binding.ty,
        Some(jet::AST::Type::Apply {
            name: "NumericBox".to_string(),
            args: vec![jet::AST::Type::Float],
        }),
        "sema must retain the concrete generic binding identity"
    );
    let tir = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("concrete generic derive lowers to resident JIT TIR");
    let numeric_init_ty = tir
        .funcs
        .iter()
        .find(|func| func.name == "run")
        .and_then(|func| {
            func.body.iter().find_map(|stmt| match stmt {
                jet::Codegen::TIR::TStmt::Let { name, init, .. } if name == "numeric" => {
                    Some(init.ty.clone())
                }
                _ => None,
            })
        })
        .expect("numeric TIR binding");
    assert_eq!(numeric_init_ty, numeric_binding.ty.clone().unwrap());
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Int>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Float>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Bool>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Char>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<String>::get_value"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<Int>::make"));
    assert!(tir.funcs.iter().any(|func| func.name == "Box<String>::make"));
    assert!(tir.funcs.iter().any(|func| func.name == "StaticOnly<Int>::marker"));
    assert!(tir.funcs.iter().any(|func| func.name == "StaticOnly<String>::marker"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::replace"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::plus"));
    assert!(tir.funcs.iter().any(|func| func.name == "NumericBox<Float>::equal_to"));
    assert!(
        tir.funcs.iter().all(|func| {
            !func.name.starts_with("Box<T>::") && !func.name.starts_with("Box<U>::")
        }),
        "abstract field types must not become fake JIT instances: {:?}",
        tir.funcs.iter().map(|func| &func.name).collect::<Vec<_>>()
    );
    let generic_method_file = dir.join("generic_method_shadow.jet");
    fs::write(
        &generic_method_file,
        r#"
derive T.GenericMethod {
    fn keep<T>(self, value: ^T) => T :: value
}
#GenericMethod
struct Shadow<T: Printable> { value: T }
fn run() {
    item := Shadow<Int>.{ value: 1 }
    print(item.value)
}
"#,
    )
    .unwrap();
    let generic_method_bundle =
        checked_bundle_from_path(generic_method_file.to_str().unwrap());
    let generic_method_tir = jet::Codegen::TIR::lower_jit_program(&generic_method_bundle)
        .expect("generic-method fixture lowers around the unsupported method");
    assert!(
        generic_method_tir
            .funcs
            .iter()
            .all(|func| !func.name.ends_with("::keep")),
        "method-owned generic T must not be captured by the owner's T substitution"
    );
    let resident = run_cranelift_without_fallback(src, "generic_user_derive");
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed generic user derive: {diags:?}"),
    };
    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("forced interpreter failed generic user derive: {diags:?}")
        }
    };
    let aot = compiled_binary_output(
        &dir,
        "generic_user_derive",
        0,
        "generic_user_derive",
        &shown,
    );
    assert_eq!(resident, expected);
    assert_eq!(default, expected);
    assert_eq!(interpreted, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn nested_generic_user_derive_reaches_resident_jit() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
derive T.Access {
    info :: T.reflect()
    param :: info.type_params[0].name
    fn get_value(self) => @param :: ~self.value
}

#Access
struct Inner<T: Printable> { value: T }

struct Outer<T: Printable> {
    value: T

    fn read(self) => T {
        inner := Inner<T>.{ value: ~self.value }
        return inner.get_value()
    }
}

fn run() {
    outer := Outer<Int>.{ value: 7 }
    print(outer.read())
}
"#;
    let output = run_cranelift_without_fallback(src, "nested_generic_user_derive");
    assert_eq!(output, ProgramOutput::ran("7\n".into(), "".into(), 0));
}

#[test]
fn unused_expanding_generic_body_does_not_expand_jit_worklist() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
struct Grow<T: Printable> {
    value: T

    fn read(self) => T { return ~self.value }
    fn unused(self) => Int {
        nested := Grow<[T]>.{ value: [~self.value] }
        return nested.unused()
    }
}

fn run() {
    value := Grow<Int>.{ value: 7 }
    print(value.read())
}
"#;
    let output = run_cranelift_without_fallback(src, "unused_expanding_generic_body");
    assert_eq!(output, ProgramOutput::ran("7\n".into(), "".into(), 0));

    let reachable = src.replace("print(value.read())", "print(value.unused())");
    let file = std::env::temp_dir().join("reachable_expanding_generic_method.jet");
    fs::write(&file, reachable).unwrap();
    let bundle = checked_bundle_from_path(file.to_str().unwrap());
    let error = jet_jit::try_compile_bundle(&bundle).unwrap_err();
    assert!(error.contains("E0909: generic instantiation goes too deep"), "{error}");
}

#[test]
fn nested_ordinary_module_generic_instance_matches_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() { return; }
    let src = r#"
module outer<T>(n: Int) {
    module plain {
        module inner<U> { pub fn total(value: U) => Int { return n } }
        module closed :: inner<T>
        pub fn result(value: T) => Int { return closed.total(value) }
    }
    pub fn result(value: T) => Int { return plain.result(value) }
}
module selected :: outer<Int>(6)
fn run() { print(selected.result(1)) }
"#;
    let jit = run_cranelift_without_fallback(src, "nested_ordinary_generic_module");
    assert_eq!(jit.stdout, "6\n");

    let dir = std::env::temp_dir().join(format!(
        "jet_nested_ordinary_generic_module_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nested_ordinary_generic_module.jet");
    fs::write(&file, src).unwrap();
    let aot = compiled_binary_output(
        &dir,
        "nested_ordinary_generic_module",
        0,
        "nested_ordinary_generic_module",
        file.to_str().unwrap(),
    );
    assert_eq!(jit, aot);
}

#[test]
fn solver_state_transitions_match_aot_in_resident_jit() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let source_path = "examples/features/tooling/solve_puzzle.jet";
    let src = fs::read_to_string(source_path).expect("read solve_puzzle example");
    let jit = run_cranelift_without_fallback(&src, "solve_puzzle");

    let compiled = jet::compile_with_path(&src, source_path).expect("compile solve_puzzle");
    let dir = std::env::temp_dir().join(format!("jet_solver_jit_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let rs = dir.join("solve_puzzle.rs");
    let bin = dir.join("solve_puzzle");
    let mut command = Command::new("rustc");
    add_generated_rust(
        &mut command,
        &rs,
        &compiled.rust,
        compiled.ffi.is_some(),
        &[],
    );
    let rustc = command.arg("-o").arg(&bin).output().expect("run rustc");
    assert!(
        rustc.status.success(),
        "rustc failed: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let output = Command::new(&bin).output().expect("run AOT solve_puzzle");
    let aot = ProgramOutput::ran(
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(1),
    );
    assert_eq!(jit, aot, "Solver state drifted between resident JIT and AOT");
    assert_eq!(
        jit.stdout,
        "key=1 door=3\nkey=3 door=1\nwins=2\nstatus=failed\nfailures=7\n"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resident_jit_result_abi_covers_calls_ok_err_try_and_entry() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let success = r#"
fn choose_ok() => Float ? String {
    return Ok(0.25)
}

fn choose_err() => Float ? String {
    return Err("typed boom")
}

fn forward() => Float ? String {
    value :: choose_ok()?
    return Ok(value + 0.25)
}

fn run() ? {
    print(forward()?)
}
"#;
    let success_jit = run_cranelift_outcome(success, "result_success");
    assert_eq!(success_jit, ProgramOutput::ran("0.5\n".into(), "".into(), 0));

    let failure = success.replace("choose_ok()?", "choose_err()?");
    let failure_jit = run_cranelift_outcome(&failure, "result_failure");
    // `run_cranelift_outcome` writes the fixture at this exact path, and every
    // journey frame names it, so the expectation must be built from it.
    let failure_shown = std::env::temp_dir()
        .join("jet_jit_result_result_failure.jet")
        .to_string_lossy()
        .into_owned();
    // D-FAIL-CTX1=A (ratified 2026-08-06, card #1532): "Each `?` hop joins the
    // failure journey on every tier, whether it has a note or not." That
    // authorises the two E3002 hops — `forward`'s `?` on line 11 and `run`'s on
    // line 16 — origin first under the root failure, spelled the way E3002 now
    // registers it: `  {n}. {fn} ({file}:{line})`.
    // D-FAIL-ERROR1=A (card #1528) + D-FAIL-EXIT1=A (card #1533): bare `fn run() ?`
    // means `run() ? Err`, so the `String` error arrives as the default error and
    // the process edge prints one full report and exits 1. `jet_render_err`
    // renders `Error: {message}` with no code and `Error [{code}]: {message}`
    // with one. Same shape as the ratified AOT golden
    // `examples/features/expected/errors/error_context.err.out`.
    assert_eq!(
        failure_jit,
        ProgramOutput::ran(
            String::new(),
            format!(
                "Error: typed boom\n\
                 \x20Trail [E3002] (2 hops via ?, origin first):\n\
                 \x20 1. forward ({failure_shown}:11)\n\
                 \x20 2. run ({failure_shown}:16)\n"
            ),
            1
        )
    );

    // Journey frames name the fixture path (D-FAIL-CTX1=A), so the interpreter
    // must read the *same* file the JIT ran; a separate `*_interp` temp name
    // would diff on the path instead of on semantics.
    for (src, tag, expected) in [
        (success, "result_success", success_jit),
        (&failure, "result_failure", failure_jit),
    ] {
        let p = std::env::temp_dir().join(format!("jet_jit_result_{tag}.jet"));
        fs::write(&p, src).unwrap();
        let shown = p.to_string_lossy().to_string();
        let interpreted = match dev_iteration(&shown, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("`{tag}` interpreter failed: {ds:?}"),
        };
        assert_eq!(interpreted, expected, "JIT/interpreter Result drift for `{tag}`");
    }
}

#[test]
fn resident_jit_fallible_void_cfg_fallthrough_matches_aot() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let one_arm_fallthrough = r#"
fn direct_ok() => Int ? {
    return Ok(7)
}

fn run() ? {
    print(direct_ok()?)
    stop :: false
    if stop {
        return Err("one-arm stopped")
    }
    print("one-arm fallthrough")
}
"#;
    let nested_fallthrough = r#"
fn direct_ok() => Int ? {
    return Ok(7)
}

fn run() ? {
    print(direct_ok()?)
    outer :: true
    inner :: false
    if outer {
        if inner {
            return Err("nested stopped")
        }
    }
    print("nested fallthrough")
}
"#;
    let neither_arm_terminates = r#"
fn direct_ok() => Int ? {
    return Ok(7)
}

fn run() ? {
    print(direct_ok()?)
    if true {
        print("left continues")
    } else {
        print("right continues")
    }
    print("neither terminated")
}
"#;
    let both_arms_terminate = r#"
fn direct_ok() => Int ? {
    return Ok(7)
}

fn run() ? {
    print(direct_ok()?)
    if true {
        return Err("left branch")
    } else {
        return Err("right branch")
    }
}
"#;

    let cases = [
        (
            "one_arm_fallthrough",
            one_arm_fallthrough,
            ProgramOutput::ran("7\none-arm fallthrough\n".into(), "".into(), 0),
        ),
        (
            "nested_fallthrough",
            nested_fallthrough,
            ProgramOutput::ran("7\nnested fallthrough\n".into(), "".into(), 0),
        ),
        (
            "neither_arm_terminates",
            neither_arm_terminates,
            ProgramOutput::ran(
                "7\nleft continues\nneither terminated\n".into(),
                "".into(),
                0,
            ),
        ),
        (
            "both_arms_terminate",
            both_arms_terminate,
            // D-FAIL-ERROR1=A (card #1528) + D-FAIL-EXIT1=A (card #1533): bare
            // `fn run() ?` means `run() ? Err`, so `return Err("left branch")`
            // reaches the process edge as one full default-error report and exits
            // 1. `jet_render_err` renders `Error: {message}` when the error
            // carries no code — see the ratified golden
            // `examples/features/expected/errors/default_err_edge.err.out`, which
            // shows the `Error [CODE]: …` form of the same renderer. No journey
            // frame here: D-FAIL-CTX1=A appends a frame per `?` hop, and this
            // `Err` is returned directly rather than re-raised.
            ProgramOutput::ran("7\n".into(), "Error: left branch\n".into(), 1),
        ),
    ];

    for (i, (tag, src, expected)) in cases.into_iter().enumerate() {
        let jit = run_cranelift_without_fallback(src, tag);
        assert_eq!(jit, expected, "resident JIT CFG result drift for `{tag}`");

        let path = std::env::temp_dir().join(format!("jet_jit_{tag}.jet"));
        fs::write(&path, src).unwrap();
        let shown = path.to_string_lossy().to_string();
        let dir = std::env::temp_dir().join(format!("jet_jit_{tag}_{}", std::process::id()));
        let aot = compiled_binary_output(&dir, tag, i, tag, &shown);
        assert_eq!(aot, expected, "AOT CFG result drift for `{tag}`");
        assert_eq!(jit, aot, "AOT and resident JIT CFG semantics drift for `{tag}`");
    }
}

#[test]
fn resident_jit_fidelity_matches_runtime_contract() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let valid = r#"
use core.perf as perf

fn run() ? {
    perf.reset_fidelity()
    print(perf.default_fidelity())
    perf.override_fidelity(0.25)?
    print(perf.fidelity())
    perf.reset_fidelity()
    print(perf.fidelity())
}
"#;
    let expected_valid = ProgramOutput::ran("1.0\n0.25\n1.0\n".into(), "".into(), 0);
    assert_eq!(run_cranelift_outcome(valid, "fidelity_valid"), expected_valid);
    let valid_path = std::env::temp_dir().join("jet_jit_fidelity_valid_interp.jet");
    fs::write(&valid_path, valid).unwrap();
    let valid_shown = valid_path.to_string_lossy().to_string();
    let interpreted = match dev_iteration(&valid_shown, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => {
            ProgramOutput::ran(stdout, stderr, exit_code)
        }
        RunOutcome::Problems(ds) => panic!("fidelity interpreter failed: {ds:?}"),
    };
    assert_eq!(interpreted, expected_valid);
    let aot_dir = std::env::temp_dir().join(format!("jet_jit_fidelity_aot_{}", std::process::id()));
    assert_eq!(
        compiled_binary_output(&aot_dir, "fidelity", 0, "fidelity", &valid_shown),
        expected_valid
    );

    for (value, tag) in [
        ("-0.01", "negative"),
        ("1.01", "above_one"),
        ("(1.0 / 0.0)", "infinite"),
        ("(0.0 / 0.0)", "nan"),
    ] {
        let src = format!(
            r#"use core.perf as perf
fn run() ? {{
    perf.reset_fidelity()
    perf.override_fidelity(0.375)?
    perf.override_fidelity({value})?
}}"#
        );
        let got = run_cranelift_outcome(&src, tag);
        assert_eq!(got.exit_code, 1, "{tag} must fail");
        assert!(
            got.stderr
                .contains("core.perf.Perf.override_fidelity needs 0.0 through 1.0"),
            "{tag}: {:?}",
            got.stderr
        );
        let read = r#"use core.perf as perf
fn run() { print(perf.fidelity()) }"#;
        assert_eq!(
            run_cranelift_outcome(read, &format!("{tag}_state")),
            ProgramOutput::ran("0.375\n".into(), "".into(), 0),
            "{tag} changed fidelity before returning Err"
        );
    }
}

/// The AST boundary must not intercept a raw-memory `#Unsafe` region, and the
/// canonical TIR evaluator must then EXECUTE it: `Ptr.from_addr` and postfix
/// `p.*` both have evaluator arms (`eval/exprs.rs` `TExprKind::PtrFromAddr` /
/// `TExprKind::Deref`), so a provenance-less address is not a coverage gap.
///
/// D-MEM-SENTRY1 fixes what it IS: a located R0801 program-side stop at exit
/// 70 (I2), identical on every tier — `tests/tir_unsafe_and_runtime.rs`
/// `sentry_faults_are_tier_parity` pins the same contract for the forced
/// interpreter, the default JIT, and AOT. The earlier expectation here (an
/// E2201 whose text carried `PtrFromAddr`) named a TIR lowering-failure reason
/// string (`lower/expressions.rs` `expr_kind_name`), and since card #2001 no
/// lowering reason reaches E2201 at all: `run_bundle_at_stage` keeps E2201 only
/// for the two no-entry reasons and sends every other one down the ICE rail.
/// So that combination is unproducible by design. This asserts the shipped
/// outcome instead, and pins strictly more of it: the outcome kind, the exit
/// status, an empty stdout (the deref never printed), and the R0801 code, gate
/// reason, provenance detail and obligation clause.
#[test]
fn unsafe_blocks_are_evaluated_by_canonical_tir_with_live_sentries() {
    let raw = jet::Loader::load_entry(&example_path("memory/rawptr"))
        .expect("rawptr example should load");
    assert!(
        jet_driver::InterpreterBoundary::dev_boundary_scan(&raw).is_none(),
        "sema-approved #Unsafe blocks are evaluated by canonical TIR"
    );

    let unsupported_path = std::env::temp_dir().join(format!(
        "jet_unsafe_tir_boundary_{}.jet",
        std::process::id()
    ));
    fs::write(
        &unsupported_path,
        "use core.mem\nfn run() {\n    #Unsafe(\"mapped address is valid and aligned\") {\n        p :: mem.Ptr<Int>.from_addr(0x40000100)\n        print(p.*)\n    }\n}\n",
    )
    .unwrap();
    let unsupported_file = unsupported_path.to_string_lossy().into_owned();
    let unsupported = jet::Loader::load_entry(&unsupported_file)
        .expect("pointer cast example should load");
    assert!(
        jet_driver::InterpreterBoundary::dev_boundary_scan(&unsupported).is_none(),
        "the AST boundary must not intercept an unsupported unsafe operation"
    );
    match dev_iteration(&unsupported_file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(
                exit_code, 70,
                "a raw read with no allocation provenance is a program-side stop: {stderr}"
            );
            assert!(
                stdout.is_empty(),
                "the refused deref must not have printed: {stdout:?}"
            );
            for marker in [
                "Runtime fault [R0801]",
                "mapped address is valid and aligned",
                "no live allocation contains this address",
                "obligation `valid_ptr` was not met on this run",
            ] {
                assert!(
                    stderr.contains(marker),
                    "interpreter sentry report is missing `{marker}`: {stderr}"
                );
            }
        }
        outcome => panic!(
            "a sema-approved #Unsafe region must be evaluated by canonical TIR, \
             not refused at a boundary: {outcome:?}"
        ),
    }
}

#[cfg(unix)]
#[test]
fn io_style_raw_nonunicode_no_color_uses_presence_semantics() {
    use std::os::unix::ffi::OsStringExt;

    std::thread::Builder::new()
        .spawn(|| {
            let src =
                "use core.term as io\nfn run() {\n    print(io.style(\"red\", \"plain\"))\n}\n";
            let dir = common::unique_tmp("jet_raw_no_color");
            fs::create_dir_all(&dir).unwrap();
            let input = dir.join("raw_no_color.jet");
            fs::write(&input, src).unwrap();
            let compiled = jet::compile_with_path(src, input.to_str().unwrap())
                .expect("raw NO_COLOR fixture must compile");
            let rust = dir.join("raw_no_color.rs");
            let bin = dir.join("raw_no_color_test");
            let probe = r#"

// PROVES: the colour decision honours `NO_COLOR` by PRESENCE. The variable is
// set here to a single 0xff byte -- present, and not valid Unicode -- and the
// environment half of the production decision still says "no colour". It also
// proves the decision reads the raw logical-env entry: the decoding accessor
// reports the same variable absent, so routing the decision through it would
// silently re-enable colour (the #1206 review defect).
//
// DOES NOT PROVE: that colour is emitted when `NO_COLOR` is absent. The test
// harness captures this program's stdout with a pipe, so the stream half of the
// decision (`jet_term_stdout_is_terminal`) is false here for an unrelated
// reason. That is precisely why the environment half is asserted through its
// own seam, and why the composed check below hands `jet_term_style_enabled` a
// forced `stdout_is_terminal = true`: asserting `jet_style_enabled() == false`
// alone would pass even if `NO_COLOR` were ignored outright. Terminal-attached
// behaviour belongs to tests/terminal.rs and the io/terminal_parity ledger.
//
// Called from `main` right after `jet_std_env_init()`. A failure panics, so the
// program exits non-zero and the entry never prints.
fn jet_probe_no_color_presence() {
    // The raw logical-env lookup the colour decision uses sees the variable.
    assert!(jet_env_value_raw("NO_COLOR").is_some());
    // The decoding accessor calls the very same variable absent.
    assert!(jet_std_env_get(&"NO_COLOR".to_string()).is_none());

    // The env-only seam: no terminal involved, so this cannot go green because
    // stdout happens to be a pipe.
    assert!(!jet_style_env_enabled());

    // The production decision itself, with only the stream fact substituted.
    // `jet_style_enabled` is `jet_term_style_enabled(no_color, term_is_dumb,
    // jet_term_stdout_is_terminal())` over these exact facts, so switching
    // either read to the decoding accessor flips this assertion.
    let (no_color, term_is_dumb) = jet_style_env_facts();
    assert!(no_color, "set-but-non-Unicode NO_COLOR must register as present");
    assert!(!jet_term_style_enabled(no_color, term_is_dumb, true));

    // End to end through the user-facing surface: no escape codes.
    let styled = jet_std_io_style(&"red".to_string(), &"plain".to_string());
    assert_eq!(styled, "plain");
}
"#;
            // The probe runs from the generated program's own `main`, NOT from a
            // `rustc --test` harness. `--test` turns `cfg(test)` on for the whole
            // generated crate, which activates every Prelude fragment's private
            // unit-test module at one crate root -- and `PRELUDE_PARTS`
            // (Codegen/mod.rs:157) unconditionally emits both
            // `Prelude/Core/Progress.rs` and `Prelude/NumericWiden.rs`, each of
            // which declares a bare `mod tests`. That is E0428, in every generated
            // program, since Progress.rs gained its module on 2026-08-04 (d0098d284;
            // NumericWiden.rs got the first one 2026-07-29, e61c31131). No shipped
            // consumer sees it -- `jet test` builds its own harness (TEST_PRELUDE /
            // `jet_test_print`), and tests/common/mod.rs:189 already calls raw
            // `--test` on generated Rust "an uncommon inspection mode ... not the
            // Jet test-harness build path". So the collision is a Prelude hygiene
            // defect worth its own card, and this probe simply stops needing the
            // mode that exposes it.
            //
            // `jet_std_env_init();` is the first statement of every generated
            // `main` (all four emitters: Codegen/mod.rs:3416, 3566, 4621, 4978), and
            // tests/env_overlay.rs:118 pins that exact text. Hooking after it means
            // the logical env table is already seeded when the probe reads it. The
            // replacement count is asserted, so a drift in generated `main` fails
            // loudly here instead of silently skipping the probe.
            const MAIN_ANCHOR: &str = "fn main() {\n    jet_std_env_init();";
            assert_eq!(
                compiled.rust.matches(MAIN_ANCHOR).count(),
                1,
                "generated `main` no longer starts with `jet_std_env_init();` -- \
                 re-anchor this probe, do not skip it"
            );
            let hooked = compiled.rust.replacen(
                MAIN_ANCHOR,
                &format!("{MAIN_ANCHOR}\n    jet_probe_no_color_presence();"),
                1,
            );
            let generated = format!("{hooked}{probe}");
            // Build the whole generated crate as ONE crate. `add_generated_rust`
            // only forces the inline (single-crate) form for `--test` or FFI
            // builds; any other flag set splits `PRELUDE_PARTS` into a cached
            // runtime rlib, which would put `jet_term_style_enabled` in another
            // crate where the probe cannot see it. This fixture has no FFI, so an
            // inline build needs no `--extern` and no cache.
            assert!(compiled.ffi.is_none(), "raw NO_COLOR fixture must not need FFI");
            fs::write(&rust, &generated).unwrap();
            let built = Command::new("rustc")
                .args(["--edition", "2021", "--crate-name", "raw_no_color"])
                .arg(&rust)
                .arg("-o")
                .arg(&bin)
                .output()
                .unwrap();
            assert!(
                built.status.success(),
                "rustc rejected raw NO_COLOR probe:\n{}",
                String::from_utf8_lossy(&built.stderr)
            );
            let run = Command::new(&bin)
                .env("NO_COLOR", std::ffi::OsString::from_vec(vec![0xff]))
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "raw NO_COLOR presence probe failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
            // The probe ran before the Jet entry, and the entry's own output is the
            // end-to-end half: `io.style("red", "plain")` carries no escape codes.
            assert_eq!(String::from_utf8_lossy(&run.stdout), "plain\n");
            let _ = fs::remove_dir_all(dir);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn jit_1216_adversarial_regressions() {
    const CASE: &str = "JET_1216_ADVERSARIAL_CASE";
    if let Ok(case) = std::env::var(CASE) {
        let source = match case.as_str() {
            "oob" => "fn run() { xs := [1]\n xs[4] = 2 }\n",
            "stm_return" => r#"
struct Slot { value: Int }
fn rollback(cell: Shared<Slot>) {
    #Transact(tx) {
        cell.value = 9
        return
    }
}
fn run() {
    cell :: shared Slot{value: 1}
    rollback(cell)
    print(cell.value)
}
"#,
            "generator" => r#"
fn stopped() => Stream<Int> {
    yield 1
    yield 2
}
fn closes() => Stream<Int> {
    yield 3
    return
}
fn run() {
    loop value, stopped() {
        print(value)
        break
    }
    loop value, closes() { print(value) }
    print("done")
}
"#,
            "raw_alias" => r#"
use core.mem
fn run() {
    value := 4
    #Unsafe("the pointer stays inside this stack frame") {
        pointer :: *Int.{*value}
        mem.volatile_write(pointer, 9)
        print(value)
    }
}
"#,
            "option_minus_one" => r#"
fn run() {
    queue := PriorityQueue.from([-1])
    print(queue.pop())
    print(queue.pop())
}
"#,
            "sum_overflow" => r#"
fn run() {
    print([9223372036854775807, 1].sum())
}
"#,
            _ => panic!("unknown #1216 adversarial case `{case}`"),
        };
        jet_jit::reset_jit_trace_for_test();
        let outcome = run_cranelift_outcome_without_fallback(source, &format!("1216_{case}"));
        match case.as_str() {
            "oob" | "sum_overflow" => {
                // Live arithmetic and bounds traps use the registered E3010 stop.
                let RunOutcome::Ran {
                    stdout: _,
                    stderr,
                    exit_code,
                } = outcome
                else {
                    panic!("`{case}` expected runtime trap Ran, got: {outcome:?}");
                };
                assert_eq!(
                    exit_code, 70,
                    "`{case}` must exit 70: err={stderr}"
                );
                assert!(
                    stderr.contains("Stop [E3010]"),
                    "`{case}` must use the E3010 stop, got: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "`{case}` must not use comptime voice: {stderr}"
                );
                if case == "oob" {
                    assert!(
                        stderr.contains("the list has 1 items, so position 4 doesn't exist"),
                        "`oob` trap wording: {stderr}"
                    );
                } else {
                    assert!(
                        stderr.contains("overflow") || stderr.contains("overflowed"),
                        "`sum_overflow` trap wording: {stderr}"
                    );
                }
            }
            expected_case => {
                let RunOutcome::Ran { stdout, .. } = outcome else {
                    panic!("`{expected_case}` did not run in resident JIT: {outcome:?}");
                };
                let expected = match expected_case {
                    "stm_return" => "1\n",
                    "generator" => "1\n3\ndone\n",
                    "raw_alias" => "9\n",
                    "option_minus_one" => "-1\nnull\n",
                    _ => unreachable!(),
                };
                assert_eq!(stdout, expected);
                assert!(jet_jit::jit_executed_for_test());
                if expected_case == "raw_alias" {
                    // D-MEM-SENTRY1=A (ratified 2026-08-12, card #1889) is an
                    // owner-ratified I9 instrumentation carve-out: raw memory stays
                    // on canonical TIR so the Prelude witness owns gate state,
                    // provenance, quarantine, poison and R08xx reporting, and
                    // Cranelift only marshals safe values rather than growing a
                    // second memory policy. `raw_alias` is `*Int.{*value}` plus a
                    // core.mem call, i.e. exactly the two shapes the walker now
                    // refuses, so the correct route is a silent deopt to canonical
                    // TIR -- not native execution. This case was authored under
                    // #1216 on 2026-07-27, sixteen days before that ratification.
                    assert!(
                        jet_jit::deopt_invoked_for_test(),
                        "`raw_alias` must take the D-MEM-SENTRY1 canonical-TIR route"
                    );
                    assert!(
                        !jet_jit::fallback_invoked_for_test(),
                        "`raw_alias` deopt must not trip forbidden fallback"
                    );
                } else {
                    assert!(!jet_jit::deopt_invoked_for_test());
                }
            }
        }
        return;
    }
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    for case in [
        "oob",
        "stm_return",
        "generator",
        "raw_alias",
        "option_minus_one",
        "sum_overflow",
    ] {
        let mut command = Command::new(std::env::current_exe().expect("current dev test binary"));
        command
            .args(["--exact", "jit_1216_adversarial_regressions", "--nocapture"])
            .env(CASE, case)
            .env("NO_COLOR", "1");
        let output = command_output_with_timeout(
            command,
            Duration::from_secs(10),
            &format!("#1216 adversarial `{case}`"),
        );
        assert!(
            output.status.success(),
            "{case}: stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn uninit_fixed_mutating_borrow_matches_interpreter_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let source = r#"
use core.mem

fn set_first(bytes: &[U8#2]) {
    bytes[0] = 8
}

fn first(bytes: [U8#2]) => U8 {
    index :: 0
    return bytes[index]
}

fn run() {
    bytes := [U8#2].{ uninit }
    bytes[0] = 1
    bytes[1] = 2
    set_first(&bytes)
    print(bytes[0])
    print(first(bytes))
}
"#;
    let file = std::env::temp_dir().join(format!(
        "jet_uninit_fixed_mutating_borrow_{}.jet",
        std::process::id()
    ));
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let expected = ProgramOutput::ran("8\n8\n".to_string(), String::new(), 0);

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced interpreter failed: {diags:?}"),
    };

    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, "uninit_fixed_mutating_borrow");
    assert!(
        jet_jit::jit_executed_for_test(),
        "uninitialized fixed-list fill did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "uninitialized fixed-list fill used deopt or fallback"
    );

    let dir = std::env::temp_dir().join(format!(
        "jet_uninit_fixed_mutating_borrow_aot_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(
        &dir,
        "uninit_fixed_mutating_borrow",
        0,
        "uninit_fixed_mutating_borrow",
        &shown,
    );

    assert_eq!(interpreted, expected, "forced interpreter output drifted");
    assert_eq!(resident, expected, "resident JIT output drifted");
    assert_eq!(aot, expected, "AOT output drifted");
    assert_eq!(resident, interpreted, "JIT and interpreter output differ");
    assert_eq!(resident, aot, "JIT and AOT output differ");

    let _ = fs::remove_file(file);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn uninit_fixed_dynamic_oob_uses_the_resident_jit_trap_path() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let source = r#"
use core.mem

fn outside() => Int {
    return 2
}

fn run() {
    bytes := [U8#2].{ uninit }
    bytes[0] = 1
    bytes[1] = 2
    print(bytes[outside()])
}
"#;
    let mut bundle = bundle_of(source, "uninit_fixed_dynamic_oob");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "{}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );

    jet_jit::reset_jit_trace_for_test();
    match run_cranelift_outcome_without_fallback(source, "uninit_fixed_dynamic_oob") {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 70, "dynamic OOB must exit 70: out={stdout} err={stderr}");
            assert!(
                stderr.contains("Stop [E3010]")
                    && stderr.contains("the list has 2 items")
                    && stderr.contains("doesn't exist"),
                "dynamic OOB trap wording: {stderr}"
            );
            assert!(
                !stderr.contains("E0953") && !stderr.contains("comptime"),
                "dynamic OOB must not use comptime voice: {stderr}"
            );
        }
        other => panic!("dynamic OOB expected runtime trap, got: {other:?}"),
    }
    assert!(jet_jit::jit_executed_for_test());
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "dynamic out-of-bounds index used deopt or fallback"
    );
}

#[test]
fn shared_scalar_edit_matches_interpreter_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let source = r#"
struct Counter { value: Int }
fn run() {
    counter :: shared Counter{value: 0}
    counter.value += 1
    print(counter.value)
}
"#;
    let file = std::env::temp_dir().join(format!(
        "jet_shared_scalar_edit_{}.jet",
        std::process::id()
    ));
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let expected = ProgramOutput::ran("1\n".to_string(), String::new(), 0);

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced interpreter failed: {diags:?}"),
    };

    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, "shared_scalar_edit");
    assert!(
        jet_jit::jit_executed_for_test(),
        "the shared field write did not execute in resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "the shared field write used deopt or fallback"
    );

    let dir = std::env::temp_dir().join(format!(
        "jet_shared_scalar_edit_aot_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let aot = compiled_binary_output(&dir, "shared_scalar_edit", 0, "shared_scalar_edit", &shown);

    assert_eq!(interpreted, expected, "forced interpreter output drifted");
    assert_eq!(resident, expected, "resident JIT output drifted");
    assert_eq!(aot, expected, "AOT output drifted");
    assert_eq!(resident, interpreted, "JIT and interpreter output differ");
    assert_eq!(resident, aot, "JIT and AOT output differ");

    let _ = fs::remove_file(file);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn comptime_scalar_examples_match_interpreter_resident_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    for stem in ["comptime/comptime_core", "comptime/comptime_tiers"] {
        let file = example_path(stem);
        let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);
        let interpreted = match dev_iteration(&file, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("interpreter failed `{stem}`: {diags:?}")
            }
        };

        let source = fs::read_to_string(&file).unwrap();
        jet_jit::reset_jit_trace_for_test();
        let resident = run_cranelift_without_fallback(&source, &stem.replace('/', "_"));
        assert!(jet_jit::jit_executed_for_test(), "`{stem}` did not execute in JIT");
        assert!(
            !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
            "`{stem}` used deopt or fallback"
        );

        let dir = std::env::temp_dir().join(format!(
            "jet_comptime_scalar_{}_{}",
            stem.replace('/', "_"),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let aot = compiled_binary_output(&dir, "comptime_scalar", 0, stem, &file);

        assert_eq!(interpreted, expected, "interpreter drifted for `{stem}`");
        assert_eq!(resident, expected, "resident JIT drifted for `{stem}`");
        assert_eq!(aot, expected, "AOT drifted for `{stem}`");
        let _ = fs::remove_dir_all(&dir);
    }

    let source = r#"
@f32_nan :: F32.NAN
@f32_inf :: F32.INFINITY
@f32_neg_inf :: F32.NEG_INFINITY
@f64_nan :: Float.NAN
@f64_inf :: Float.INFINITY
@f64_neg_inf :: Float.NEG_INFINITY

fn run() {
    print(@f32_nan)
    print(@f32_inf)
    print(@f32_neg_inf)
    print(@f64_nan)
    print(@f64_inf)
    print(@f64_neg_inf)
}
"#;
    let expected = ProgramOutput::ran("NaN\ninf\n-inf\nNaN\ninf\n-inf\n".into(), "".into(), 0);
    let dir =
        std::env::temp_dir().join(format!("jet_comptime_nonfinite_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("comptime_nonfinite.jet");
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("interpreter failed nonfinite scalars: {diags:?}"),
    };
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, "comptime_nonfinite");
    assert!(jet_jit::jit_executed_for_test());
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "nonfinite scalar fixture used deopt or fallback"
    );
    let aot = compiled_binary_output(
        &dir,
        "comptime_nonfinite",
        0,
        "comptime_nonfinite",
        &shown,
    );

    assert_eq!(interpreted, expected);
    assert_eq!(resident, expected);
    assert_eq!(aot, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generic_modules_full_example_matches_resident_jit_and_aot() {
    let dir = common::unique_tmp("jet_generic_modules_full_example");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("generic_modules.jet");
    fs::copy("examples/features/modules/generic_modules.jet", &file).unwrap();
    assert_cranelift_three_way(file.to_str().unwrap(), "modules/generic_modules");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn array_of_structs_field_mutation_three_way() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/collections/struct_list_mutation.jet";
    // Tiered/default path (and three-way) cover IndexFieldAssign via Cranelift;
    // pure-interpreter coverage is optional (#779 expands TIR assign arms).
    assert_cranelift_three_way(file, "collections/struct_list_mutation");
}

#[test]
fn place_windows_matches_resident_jit_and_aot_without_fallback() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stem = "memory/place_windows";
    let file = example_path(stem);
    let expected = ProgramOutput::ran(golden_stdout(stem), String::new(), 0);
    // The ratchet here used to require the forced interpreter to STOP at
    // E2201. It runs the example now, so that claim is stale: card 2001
    // (c86f848ed) stopped `run_bundle_at_stage` reporting every lowering
    // failure as a missing `run`, and the place-loan mechanism reached
    // `__JetViewMut` regions — this example's own subject.
    //
    // A boundary that has fallen is not weakened into silence. The interpreter
    // now owes the same golden every other tier owes here, which is strictly
    // more than "it must refuse" ever proved, and it is what the corpus sweep
    // already measures for this stem (`check_interpreter_stem`, no manifested
    // divergence entry).
    let interpreted = match dev_iteration(&file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("`{stem}` must run in the forced interpreter: {diags:?}")
        }
    };

    let source = fs::read_to_string(&file).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(&source, "place_windows");
    assert!(
        jet_jit::jit_executed_for_test(),
        "`{stem}` did not execute in JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "`{stem}` used deopt or fallback"
    );

    let dir = std::env::temp_dir().join(format!("jet_place_windows_{}", std::process::id()));
    let aot = compiled_binary_output(&dir, "place_windows", 0, stem, &file);
    assert_eq!(interpreted, expected, "interpreter drifted for `{stem}`");
    assert_eq!(resident, expected, "resident JIT drifted for `{stem}`");
    assert_eq!(aot, expected, "AOT drifted for `{stem}`");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fixed_width_integers_match_interpreter_resident_jit_default_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    // Two retired spellings, both from before this fixture's neighbours moved.
    // D-ARROW-CONTROL1: a callable result is `=>`; `->` is reserved for
    // selected or yielded control values, and sema rejects it with E0070, so
    // every signature below stopped type-checking and the forced interpreter
    // reported nine E0070s instead of running. D-EXPOP1=A / D-XORSPELL1=A
    // (2026-08-05): the caret raises to a power and exclusive-or is `~|`, so
    // `flags ^ mask` on a trapping `U8` would overflow rather than answer the
    // 7 this block asks for. Both spellings move; not one expected value does.
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let source = r#"
fn i8_id(value: I8) => I8 { return value }
fn i16_id(value: I16) => I16 { return value }
fn i32_id(value: I32) => I32 { return value }
fn i64_id(value: I64) => I64 { return value }
fn u8_id(value: U8) => U8 { return value }
fn u16_id(value: U16) => U16 { return value }
fn u32_id(value: U32) => U32 { return value }
fn u64_id(value: U64) => U64 { return value }
fn pass_u64(value: U64?) => (U64?) { return ~value }

fn run() {
    print(i8_id(I8.{-8}))
    print(i16_id(I16.{-1600}))
    print(i32_id(I32.{-320000}))
    print(i64_id(-6400000000))
    print(u8_id(U8.{8}))
    print(u16_id(U16.{1600}))
    print(u32_id(U32.{320000}))
    maximum :: u64_id(U64.MAX)
    print(maximum)
    print("{maximum}")
    print(maximum.to_string())
    print("{maximum:Debug}")
    print([maximum, U64.{1}])
    print([U64#2].{maximum, U64.{1}})
    print(-i8_id(I8.{8}))
    print(-i16_id(I16.{16}))
    print(-i32_id(I32.{32}))
    print(-i64_id(64))

    print(i8_id(I8.{10}) + I8.{5})
    print(i16_id(I16.{100}) - I16.{40})
    print(i32_id(I32.{7}) * I32.{6})
    print(i64_id(84) / 2)
    print(19 % 4)
    print(i8_id(I8.{7}) % I8.{3})
    flags :: u8_id(U8.{13})
    mask :: U8.{10}
    print(flags & mask)
    combined := U8.{flags}
    combined |= mask
    print(combined)
    print(flags ~| mask)
    print(flags << 1)
    print(u8_id(U8.MAX) << 1)
    print(i8_id(I8.{64}) << 1)
    print(flags >> 2)
    print(u16_id(U16.MAX) > U16.{1})
    print(u32_id(U32.MAX) > U32.{1})
    print(maximum > U64.{1})
    print(maximum >> 63)
    print(flags.count_ones())
    print(flags.count_zeros())
    print(flags.leading_zeros())
    print(flags.trailing_zeros())

    i8_max :: I8.MAX
    i8_one :: I8.{1}
    i8_zero :: I8.{0}
    print(wrapping(i8_max + i8_one))
    print(saturating(i8_max + i8_one))
    print(checked(i8_max + i8_zero) ?? i8_zero)
    print(checked(i8_max + i8_one) ?? i8_zero)
    i16_max :: I16.MAX
    i16_one :: I16.{1}
    i16_zero :: I16.{0}
    print(wrapping(i16_max + i16_one))
    print(saturating(i16_max + i16_one))
    print(checked(i16_max + i16_zero) ?? i16_zero)
    print(checked(i16_max + i16_one) ?? i16_zero)
    i32_max :: I32.MAX
    i32_one :: I32.{1}
    i32_zero :: I32.{0}
    print(wrapping(i32_max + i32_one))
    print(saturating(i32_max + i32_one))
    print(checked(i32_max + i32_zero) ?? i32_zero)
    print(checked(i32_max + i32_one) ?? i32_zero)
    i64_max :: I64.{9223372036854775807}
    i64_one :: I64.{1}
    i64_zero :: I64.{0}
    print(wrapping(i64_max + i64_one))
    print(saturating(i64_max + i64_one))
    print(checked(i64_max + i64_zero) ?? i64_zero)
    print(checked(i64_max + i64_one) ?? i64_zero)
    u8_max :: U8.MAX
    u8_one :: U8.{1}
    u8_zero :: U8.{0}
    print(wrapping(u8_max + u8_one))
    print(saturating(u8_max + u8_one))
    print(checked(u8_max + u8_zero) ?? u8_zero)
    print(checked(u8_max + u8_one) ?? u8_zero)
    u16_max :: U16.MAX
    u16_one :: U16.{1}
    u16_zero :: U16.{0}
    print(wrapping(u16_max + u16_one))
    print(saturating(u16_max + u16_one))
    print(checked(u16_max + u16_zero) ?? u16_zero)
    print(checked(u16_max + u16_one) ?? u16_zero)
    u32_max :: U32.MAX
    u32_one :: U32.{1}
    u32_zero :: U32.{0}
    print(wrapping(u32_max + u32_one))
    print(saturating(u32_max + u32_one))
    print(checked(u32_max + u32_zero) ?? u32_zero)
    print(checked(u32_max + u32_one) ?? u32_zero)
    u64_one :: U64.{1}
    u64_zero :: U64.{0}
    print(wrapping(maximum + u64_one))
    print(saturating(maximum + u64_one))
    print(checked(maximum + u64_zero) ?? u64_zero)
    print(checked(maximum + u64_one) ?? u64_zero)
    print(pass_u64(checked(maximum + u64_zero)))
    print(pass_u64(checked(maximum + u64_one)))
    print(pass_u64(checked(maximum + u64_zero)) ?? u64_zero)
    print(checked(u64_zero - u64_one) ?? maximum)
    print(checked(maximum / u64_one) ?? u64_zero)
    print(checked(maximum / u64_zero) ?? u64_zero)
    i8_negative :: I8.{-1}
    print(checked(i8_negative + i8_zero) ?? i8_zero)
}
"#;
    let expected = ProgramOutput::ran(
        concat!(
            "-8\n-1600\n-320000\n-6400000000\n8\n1600\n320000\n",
            "18446744073709551615\n18446744073709551615\n18446744073709551615\n",
            "18446744073709551615\n",
            "[18446744073709551615, 1]\n[18446744073709551615, 1]\n",
            "-8\n-16\n-32\n-64\n15\n60\n42\n42\n3\n1\n8\n15\n7\n26\n254\n-128\n3\n",
            "true\ntrue\ntrue\n1\n3\n5\n4\n0\n",
            "-128\n127\n127\n0\n",
            "-32768\n32767\n32767\n0\n",
            "-2147483648\n2147483647\n2147483647\n0\n",
            "-9223372036854775808\n9223372036854775807\n9223372036854775807\n0\n",
            "0\n255\n255\n0\n",
            "0\n65535\n65535\n0\n",
            "0\n4294967295\n4294967295\n0\n",
            "0\n18446744073709551615\n18446744073709551615\n0\n",
            "18446744073709551615\nnull\n",
            "18446744073709551615\n18446744073709551615\n",
            "18446744073709551615\n0\n-1\n",
        )
            .into(),
        String::new(),
        0,
    );
    let dir =
        std::env::temp_dir().join(format!("jet_fixed_width_integers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("fixed_width_integers.jet");
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "fixed-width integer fixture must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|reason| panic!("fixed-width integer fixture must JIT-compile: {reason}"));

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("fixed-width interpreter failed: {diags:?}"),
    };
    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, "fixed_width_integers");
    assert!(jet_jit::jit_executed_for_test());
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "fixed-width fixture used deopt or fallback"
    );
    let default = match dev_iteration(&shown, false, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("fixed-width default run failed: {diags:?}"),
    };
    let aot = compiled_binary_output(&dir, "fixed_width_integers", 0, "fixed_width", &shown);

    assert_eq!(interpreted, expected, "interpreter fixed-width drift");
    assert_eq!(resident, expected, "resident JIT fixed-width drift");
    assert_eq!(default, expected, "default fixed-width drift");
    assert_eq!(aot, expected, "AOT fixed-width drift");
    let _ = fs::remove_dir_all(&dir);

    for stem in ["lowlevel/sized_integers", "types/typed_literal_head"] {
        let example = example_path(stem);
        assert_cranelift_three_way(&example, stem);
    }
}

#[test]
fn fixed_width_signed_remainder_overflow_traps_across_tiers() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let source = r#"
fn remainder(value: I8, divisor: I8) => I8 {
    return value % divisor
}

fn run() {
    print(remainder(I8.MIN, I8.{-1}))
}
"#;
    let dir = std::env::temp_dir().join(format!(
        "jet_fixed_width_remainder_trap_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("fixed_width_remainder_trap.jet");
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "signed remainder trap fixture must stay resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|reason| panic!("signed remainder fixture must JIT-compile: {reason}"));

    // D-MODSEM1: `MIN % -1` is 0 and fits — jet_mod answers rather than traps.
    let expected = ProgramOutput::ran("0\n".into(), String::new(), 0);
    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        other => panic!("interpreter MIN % -1: {other:?}"),
    };
    jet_jit::reset_jit_trace_for_test();
    let resident = match run_cranelift_outcome_without_fallback(source, "fixed_width_remainder_trap")
    {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        other => panic!("resident JIT MIN % -1: {other:?}"),
    };
    assert!(jet_jit::jit_executed_for_test());
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "signed remainder fixture used deopt or fallback"
    );
    assert_eq!(interpreted, expected, "interpreter MIN % -1 drift");
    assert_eq!(resident, expected, "resident JIT MIN % -1 drift");

    let aot = compiled_binary_output(
        &dir,
        "fixed_width_remainder_trap",
        0,
        "fixed_width_remainder_trap",
        &shown,
    );
    assert_eq!(aot, expected, "AOT MIN % -1 presentation drift");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fixed_width_and_plain_int_remainder_zero_traps_across_tiers() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let cases = [
        (
            "fixed_width",
            r#"
fn remainder(value: I8, divisor: I8) => I8 {
    return value % divisor
}

fn run() {
    print(remainder(I8.{7}, I8.{0}))
}
"#,
        ),
        (
            "plain_int",
            r#"
fn remainder(value: Int, divisor: Int) => Int {
    return value % divisor
}

fn run() {
    print(remainder(19, 0))
}
"#,
        ),
    ];
    let dir =
        std::env::temp_dir().join(format!("jet_remainder_zero_traps_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    for (i, (tag, source)) in cases.into_iter().enumerate() {
        let file = dir.join(format!("{tag}.jet"));
        fs::write(&file, source).unwrap();
        let shown = file.to_string_lossy().to_string();
        let bundle = checked_bundle_from_path(&shown);
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "{tag} remainder-zero fixture must stay resident-safe: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
        jet_jit::try_compile_bundle(&bundle)
            .unwrap_or_else(|reason| panic!("{tag} remainder-zero fixture must JIT-compile: {reason}"));

        let interpreted = match dev_iteration(&shown, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(
                    exit_code, 70,
                    "{tag} interpreter must exit 70: out={stdout} err={stderr}"
                );
                assert!(
                    stderr.contains("Stop [E3010]") && stderr.contains("divided by zero"),
                    "{tag} interpreter trap wording: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "{tag} interpreter must not use comptime voice: {stderr}"
                );
                (stdout, stderr)
            }
            other => panic!("{tag} interpreter expected runtime trap, got: {other:?}"),
        };
        jet_jit::reset_jit_trace_for_test();
        let resident = match run_cranelift_outcome_without_fallback(source, tag) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(
                    exit_code, 70,
                    "{tag} resident JIT must exit 70: out={stdout} err={stderr}"
                );
                assert!(
                    stderr.contains("Stop [E3010]") && stderr.contains("divided by zero"),
                    "{tag} resident trap wording: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "{tag} resident must not use comptime voice: {stderr}"
                );
                (stdout, stderr)
            }
            other => panic!("{tag} resident JIT expected runtime trap, got: {other:?}"),
        };
        assert!(
            jet_jit::jit_executed_for_test(),
            "{tag} remainder-zero fixture did not execute in resident JIT"
        );
        assert!(
            !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
            "{tag} remainder-zero fixture used deopt or fallback"
        );
        assert_eq!(
            interpreted.0, resident.0,
            "{tag} stdout drift between interpret and resident"
        );
        // stderr paths may differ (source span formatting); pin the panic sentence.
        assert!(
            interpreted.1.contains("divided by zero") && resident.1.contains("divided by zero")
        );

        let aot = compiled_binary_output(&dir, tag, i, tag, &shown);
        assert_eq!(aot.exit_code, 70, "{tag} AOT must exit 70");
        assert!(
            aot.stderr.contains("Stop [E3010]") && aot.stderr.contains("divided by zero"),
            "{tag} AOT remainder-zero presentation drift: {}",
            aot.stderr
        );
        assert!(
            !aot.stderr.contains("panicked at") && !aot.stderr.contains("E0953"),
            "{tag} leaked a raw Rust panic or comptime voice"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fixed_width_mixed_sign_shift_counts_trap_across_tiers() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let cases = [
        (
            "shl_negative",
            "U8",
            "I8",
            "U8.{1}",
            "I8.{-1}",
            "<<",
            "shifting left by -1 bits is out of range (this type is 8 bits wide)",
        ),
        (
            "shr_negative",
            "U8",
            "I8",
            "U8.{1}",
            "I8.{-1}",
            ">>",
            "shifting right by -1 bits is out of range (this type is 8 bits wide)",
        ),
        (
            "shl_huge",
            "I8",
            "U64",
            "I8.{1}",
            "U64.MAX",
            "<<",
            "shifting left by 18446744073709551615 bits is out of range (this type is 8 bits wide)",
        ),
        (
            "shr_width",
            "U8",
            "U8",
            "U8.{1}",
            "U8.{8}",
            ">>",
            "shifting right by 8 bits is out of range (this type is 8 bits wide)",
        ),
    ];
    let dir =
        std::env::temp_dir().join(format!("jet_fixed_width_shift_traps_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    for (i, (tag, value_ty, count_ty, value, count, operator, trap)) in
        cases.into_iter().enumerate()
    {
        let source = format!(
            "fn shift(value: {value_ty}, count: {count_ty}) => {value_ty} {{\n    return value {operator} count\n}}\n\nfn run() {{\n    print(shift({value}, {count}))\n}}\n"
        );
        let file = dir.join(format!("{tag}.jet"));
        fs::write(&file, &source).unwrap();
        let shown = file.to_string_lossy().to_string();
        let bundle = checked_bundle_from_path(&shown);
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "{tag} must stay resident-safe: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
        jet_jit::try_compile_bundle(&bundle)
            .unwrap_or_else(|reason| panic!("{tag} must JIT-compile: {reason}"));

        let interpreted = match dev_iteration(&shown, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(
                    exit_code, 70,
                    "{tag} interpreter must exit 70: out={stdout} err={stderr}"
                );
                assert!(
                    stderr.contains("Stop [E3010]") && stderr.contains(trap),
                    "{tag} interpreter trap wording: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "{tag} interpreter must not use comptime voice: {stderr}"
                );
                (stdout, stderr)
            }
            other => panic!("{tag} interpreter expected runtime trap, got: {other:?}"),
        };
        jet_jit::reset_jit_trace_for_test();
        let resident = match run_cranelift_outcome_without_fallback(&source, tag) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(
                    exit_code, 70,
                    "{tag} resident JIT must exit 70: out={stdout} err={stderr}"
                );
                assert!(
                    stderr.contains("Stop [E3010]") && stderr.contains(trap),
                    "{tag} resident trap wording: {stderr}"
                );
                assert!(
                    !stderr.contains("E0953") && !stderr.contains("comptime"),
                    "{tag} resident must not use comptime voice: {stderr}"
                );
                (stdout, stderr)
            }
            other => panic!("{tag} resident JIT expected runtime trap, got: {other:?}"),
        };
        assert!(jet_jit::jit_executed_for_test(), "{tag} did not run natively");
        assert!(
            !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
            "{tag} used deopt or fallback"
        );
        assert_eq!(
            interpreted.0, resident.0,
            "{tag} stdout drift between interpret and resident"
        );

        let aot = compiled_binary_output(&dir, tag, i, tag, &shown);
        assert_eq!(aot.exit_code, 70, "{tag} AOT must exit 70");
        assert!(
            aot.stderr.contains("Stop [E3010]") && aot.stderr.contains(trap),
            "{tag} AOT shift trap presentation drift: {}",
            aot.stderr
        );
        assert!(
            !aot.stderr.contains("E0953") && !aot.stderr.contains("panicked at"),
            "{tag} leaked comptime voice or raw Rust panic"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn numeric_singleton_splits_match_resident_jit_and_aot_without_fallback() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    // D-EXPOP1=A / D-XORSPELL1=A (ratified 2026-08-05, syntax-decisions.md):
    // `^=` raises in place and exclusive-or-assign is `~|=`. This fixture was
    // written when the caret was xor, so `high ^= 10` had stopped meaning what
    // the golden below says: the resident tier answered 9 ^ 10 = 3486784401,
    // then `|= 8` = 3486784409, which is the RIGHT answer for the operator on
    // the page. The bit-operator coverage this block exists for is spelled
    // `~|=` now, so the operator moves and every expected value stays.
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let source = r#"
fn run() {
    values := [1.5, 2.5, 3.5]
    first :: &values[0]
    last :: &values[2]
    first = 4.5
    last = 6.5
    first += 1.25
    last += 0.25
    print(first)
    print(last)
    print(values[0])
    print(values[2])

    counts := [1, 2, 3]
    low :: &counts[0]
    high :: &counts[2]
    low += 4
    high += 6
    low &= 6
    low <<= 1
    high ~|= 10
    high |= 8
    print(low)
    print(high)
    print(counts[0])
    print(counts[2])
}
"#;
    let expected = ProgramOutput::ran(
        "5.75\n6.75\n5.75\n6.75\n8\n11\n8\n11\n".into(),
        String::new(),
        0,
    );
    let dir =
        std::env::temp_dir().join(format!("jet_numeric_singleton_splits_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("numeric_singleton_splits.jet");
    fs::write(&file, source).unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "numeric singleton splits must stay resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|reason| panic!("numeric singleton splits must JIT-compile: {reason}"));

    let RunOutcome::Problems(interpreter_diags) = dev_iteration(&shown, false, true) else {
        panic!("numeric singleton splits unexpectedly left their interpreter boundary");
    };
    assert!(
        interpreter_diags.iter().any(|diag| diag.code == "E2201"),
        "numeric singleton split interpreter boundary drifted: {interpreter_diags:?}"
    );

    jet_jit::reset_jit_trace_for_test();
    let resident = run_cranelift_without_fallback(source, "numeric_singleton_splits");
    assert!(
        jet_jit::jit_executed_for_test(),
        "numeric singleton splits did not execute in JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "numeric singleton splits used deopt or fallback"
    );

    let aot = compiled_binary_output(
        &dir,
        "numeric_singleton_splits",
        0,
        "numeric_singleton_splits",
        &shown,
    );
    assert_eq!(resident, expected, "resident JIT numeric singleton split drifted");
    assert_eq!(aot, expected, "AOT numeric singleton split drifted");
    let _ = fs::remove_dir_all(&dir);
}

/// #1989 — two independent sites, both "one type source for the host".
///
/// `Size` is a CORE struct with no TIR entry, so TIR leaves `Type::Int` on
/// `size.width` while the field lowering recovers the declared slot and emits
/// an `f64`. `emit_print`'s integer fast path used to key on the raw `inner.ty`
/// and hand that `f64` to `int_to_string`, declared `i64`.
///
/// Separately, the compiler-written `Frame::decode` decodes a `[Float#4]`
/// field: `result_payload` yields `f64` while `lower_datatree_decode_list_items`
/// hardcoded `list_push`, declared `i64`. The codec is generated for every
/// structurally eligible struct (D-META-AUTO1=A), so declaring `Frame` emits
/// that site — `require_funcs` proves it rather than assuming it.
#[test]
fn resident_jit_1989_print_and_decode_host_types_pass_the_verifier() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");

    assert_resident_clif_shape(
        "clif_1989_core_struct_float_field_print",
        r#"use core.ui as ui

fn run() {
    backend :: ui.null_backend()
    node :: ui.node("hello", 100.0, 20.0)
    constraint :: ui.constraint(0.0, 0.0, 200.0, 100.0)
    size :: backend.measure(node, constraint)
    print(size.width)
    print(size.height)
}
"#,
        &["run"],
        "100.0\n20.0\n",
    );

    assert_resident_clif_shape(
        "clif_1989_fixed_float_list_decode_push",
        r#"struct Frame {
    values: [Float#4]
}

fn run() {
    frame :: Frame.{values: [Float#4].{1.5, 2.5, 3.5, 4.5}}
    print(frame.values[3])
}
"#,
        &["Frame::decode"],
        "4.5\n",
    );
}

/// #1990 — `core.regex.flags` declares three `i64` parameters, and Jet lowers
/// `Bool` to `i8`. `lower_recorded_core_call_values` built the call straight
/// from `lower_expr` results with no reconciliation against the callee's
/// declared `AbiParam` list, so three `iconst.i8` went into `i64` slots. The
/// hand-written `core.regex` branch has widened since ec4c9c460, but the
/// registry path runs first and shadows it, which is how a real fix coexisted
/// with a live bug — so the fixture must go through `re.flags`, not a
/// hand-lowered spelling.
///
/// `rx.flags()` reads back all three marshalled arguments (`i`, `m`, no `s`),
/// and `count("A")` is 1 only if `case_insensitive` genuinely reached the
/// engine, so a truncated or garbage extension shows in stdout as well.
#[test]
fn resident_jit_1990_bool_core_call_args_pass_the_verifier() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");

    assert_resident_clif_shape(
        "clif_1990_bool_core_call_args",
        r#"use core.regex as re

fn run() {
    flag_set :: re.flags(true, true, false)
    rx :: re.compile_with("[a-z]+", flag_set) ?? panic("bad pattern")
    print(rx.flags())
    print(rx.count("A"))
}
"#,
        &["run"],
        "im\n1\n",
    );
}

/// #1991 — a sema-proved-dead edge still has to hand a value to the slot it
/// flows into. An exhaustive `Float` match lowers to nested if-let expressions
/// whose final else is `Unreachable`, and it merges on an `f64` block
/// parameter; `Unreachable` returned `iconst.i64 0` regardless, so the jump
/// passed an `i64` into an `f64` block parameter. The arms must therefore be
/// exhaustive (no `else`), and the match must yield `Float`.
#[test]
fn resident_jit_1991_dead_edge_zero_passes_the_verifier() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");

    assert_resident_clif_shape(
        "clif_1991_dead_edge_zero_merge_type",
        r#"enum Shape {
    Circle(Float)
    Square(Float)
    Empty
}

fn area(s: Shape) => Float {
    return if s == {
        .Circle(r) -> r * r
        .Square(side) -> side * side
        .Empty -> 0.0
    }
}

fn run() {
    print(area(.Circle(3.0)))
    print(area(.Square(4.0)))
    print(area(.Empty))
}
"#,
        &["area"],
        "9.0\n16.0\n0.0\n",
    );
}

#[test]
fn resident_jit_safety_detail_smoke() {
    for stem in [
        "basics/compound",
        "basics/value_dispatch",
        "types/structs",
        "types/enums",
        "basics/branches",
        "concurrency/task_group",
    ] {
        let file = format!("examples/features/{stem}.jet");
        let mut bundle = jet::Loader::load_entry(&file).expect("load");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
        let stmts = jet_jit::jit_dump_main_stmts(&bundle);
        let funcs = jet_jit::jit_program_func_names(&bundle);
        eprintln!("{stem}: {detail}");
        if stem == "basics/value_dispatch" {
            eprintln!("  compile: {:?}", jet_jit::try_compile_bundle(&bundle));
        }
        if stem == "concurrency/task_group" {
            let (sites, lams) = jet_jit::jit_spawn_stats(&bundle);
            eprintln!("  spawn: {sites} sites / {lams} lambdas");
            eprintln!(
                "  uncovered: {:?}",
                jet_jit::jit_main_uncovered_detail(&bundle)
            );
        }
        eprintln!("  funcs: {}", funcs.join(", "));
        eprintln!("  main: {}", stmts.join(", "));
        for c in jet_jit::jit_dump_mixed_switch_conds(&bundle) {
            eprintln!("  mixed: {c}");
        }
        for fn_name in ["show", "next", "label", "describe"] {
            match jet_jit::resident_jit_func_safety_detail(&bundle, fn_name) {
                jet_jit::ResidentJitSafety::Covered => {}
                jet_jit::ResidentJitSafety::Gap(d) | jet_jit::ResidentJitSafety::Unavailable(d) => {
                    eprintln!("  {fn_name}: {d}");
                }
            }
        }
        if let Err(e) = jet_jit::try_compile_bundle(&bundle) {
            eprintln!("  compile: {e}");
        }
    }
}

#[test]
fn resident_jit_safe_labeled_loop_control() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    outer :: loop i, 0..<2 {
        loop {
            if i == 0 {
                next(outer)
            }
            break(outer)
        }
    }
    print("done")
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "labeled_loop_control"),
        ProgramOutput::ran("done\n".into(), "".into(), 0)
    );
}

#[test]
fn resident_jit_named_or_fallback_loop_control() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let src = r#"
fn run() {
    values := [7]
    outer :: loop i, 0..<2 {
        loop {
            value :: values.get(1 - i) ?? next(outer)
            print(value)
            values.get(99) ?? break(outer)
        }
    }
    print("done")
}
"#;
    assert_eq!(
        run_cranelift_without_fallback(src, "named_or_fallback_loop_control"),
        ProgramOutput::ran("7\ndone\n".into(), "".into(), 0)
    );
}

#[test]
fn resident_jit_safe_increment_decrement() {
    // Front-end work belongs on Jet's canonical compiler worker
    // (`jet_foundation::CompilerStack::COMPILER_STACK_SIZE`, 64 MiB), the same
    // hop `with_jit_test_scope` makes. TIR lowering is a mutually recursive
    // descent whose debug frames are enormous — `lower_method_call_impl`
    // 528 KiB, `lower_expr_inner` 272 KiB, `lower_stmt_plan` 256 KiB — so
    // roughly two levels of method nesting exhaust libtest's 2 MiB worker
    // stack. Reaching the front end straight from a test thread aborts the
    // whole binary (SIGABRT), which reports every other in-flight test in
    // this file as failed.
    jet::run_compiler_work(|| {
        let file = "examples/features/basics/increment.jet";
        let mut bundle = jet::Loader::load_entry(file).expect("load");
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "increment example must type-check");
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "prefix/postfix ++/-- should stay JIT-covered: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
    });
}

#[test]
fn resident_jit_safe_named_tuples() {
    // Compiler worker required; see `resident_jit_safe_increment_decrement`.
    jet::run_compiler_work(|| {
        let file = "examples/features/basics/tuples.jet";
        let mut bundle = jet::Loader::load_entry(file).expect("load");
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "tuple example must type-check");
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "named tuple literal/access/equality/destructure should stay JIT-covered: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
    });
}

#[test]
fn resident_jit_safe_zip_family() {
    // Compiler worker required; see `resident_jit_safe_increment_decrement`.
    jet::run_compiler_work(|| {
        let file = "examples/features/collections/zip_family.jet";
        let mut bundle = jet::Loader::load_entry(file).expect("zip family example");
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "zip family example must type-check: {errors:#?}");
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "zip family must stay resident-safe: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
        jet_jit::try_compile_bundle(&bundle)
            .unwrap_or_else(|reason| panic!("zip family must JIT-compile: {reason}"));
    });
}

#[test]
fn resident_jit_safe_chained_comparison() {
    // Compiler worker required; see `resident_jit_safe_increment_decrement`.
    jet::run_compiler_work(|| {
        let file = "examples/features/operators/chained_comparison.jet";
        let mut bundle = jet::Loader::load_entry(file).expect("load");
        let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        let errors: Vec<_> = diags
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "chained comparison example must type-check"
        );
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "same-direction chained comparisons should stay JIT-covered: {}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
    });
}

#[test]
fn resident_jit_safe_user_operator_traits() {
    let file = "examples/features/operators/user_defined.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags.into_iter().filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)).collect();
    assert!(errors.is_empty(), "user operator example must type-check: {errors:#?}");
    assert!(jet_jit::resident_jit_safe_bundle(&bundle), "user operators should stay JIT-covered: {}", jet_jit::resident_jit_safe_bundle_detail(&bundle));
    let src = fs::read_to_string(file).expect("operator example");
    let output = run_cranelift_without_fallback(&src, "user_operator_traits");
    assert_eq!(output, ProgramOutput::ran("4,6 4,6 true true false\n".into(), "".into(), 0));
}

#[test]
fn resident_jit_safe_string_method_chain() {
    let file = "examples/features/basics/method_chain.jet";
    let mut bundle = jet::Loader::load_entry(file).expect("load");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "method-chain example must type-check");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "pure string method chains should stay JIT-covered: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

/// The corpus gate has no live run-tier parity failures.
#[test]
fn run_tier_parity_guard() {
    let run_gaps = corpus_gate_run_gaps(&parse_corpus_gate_manifest());
    assert!(
        run_gaps.is_empty(),
        "JIT run-tier parity has {} gap(s):\n{}",
        run_gaps.len(),
        run_gaps.join("\n")
    );
}

/// #1760: the two serde stems stay green — and each pin claims only what a
/// ledger can actually prove about that stem.
///
/// The old run-tier pin demanded `resident_jit:` for BOTH stems. That was true
/// the day it was written (`af3201409`) and is not true now, and neither half of
/// the change was a regression this test could have caught:
///
/// * `d1aaf936e` rewrote error handling across 39 net and serde examples one day
///   AFTER the gate last moved, so the `resident_jit:` rows for those stems were
///   never re-observed. `f7aab7bf8` re-derived the ledger from an observed run
///   (374 -> 496 rows, 122 stems previously in no section at all) and recorded
///   what the tiers actually do: `serde/encoding_breadth` is `frontend_rejected:`
///   on the E2402 `?`-conversion family, and `serde/serde_generic` is a bare
///   `deopt_interp:` row.
/// * Nothing ran this test in between. `dev` belonged to no named target set
///   until `tests/suites.txt` landed, so the pin went stale silently — the same
///   shape as the `compile_covered` claim described below.
///
/// So the pin is re-stated against the OBSERVED classification, and only in the
/// directions the gate's own ratchet allows. `run_tier_broken:` and
/// `tier_divergent:` are shrink-only and both are currently EMPTY, so "neither
/// stem may be a refusal or a divergence" can only ever get easier to satisfy —
/// it is a real parity claim, not a restatement of today's row. A bare
/// `deopt_interp:` row is a TIER CHOICE; the gate treats a detail-carrying one as
/// FAILING, so the empty-detail pin below is the
/// assertion that actually catches a `serde/serde_generic` regression, and it is
/// stricter than the class pin it replaces.
///
/// Compile coverage is owned by `jit_coverage_audit`; this test only pins the
/// two serde rows' observed run-tier classification.
#[test]
fn serde_jit_parity_manifest_pins() {
    let gate = parse_corpus_gate_manifest();
    for stem in ["serde/encoding_breadth", "serde/serde_generic"] {
        // Accounted for exactly once. A ghost row, a duplicate, or a stem that
        // dropped out of the file entirely would otherwise make every claim
        // below vacuously true.
        let rows: Vec<_> = gate.iter().filter(|record| record.stem == stem).collect();
        assert_eq!(
            rows.len(),
            1,
            "{stem} must carry exactly one corpus-gate row, found {}",
            rows.len()
        );
        let record = rows[0];
        // The parity claim, on the two sections that mean a tier disagreed.
        assert!(
            !matches!(
                record.class,
                CorpusGateClass::RunTierBroken | CorpusGateClass::TierDivergent
            ),
            "{stem} became a run-tier refusal or a tier divergence: {:?} {}",
            record.class,
            record.detail
        );
        // A tier choice carries no diagnostic; a failure does.
        if record.class == CorpusGateClass::DeoptInterp {
            assert!(
                record.detail.is_empty(),
                "{stem} deopts to the interpreter CARRYING a diagnostic: {}",
                record.detail
            );
        }
        // #2018: E2402 is repaired — the core-error family ships one
        // `impl <CoreError> => Err` on the D-FAIL-CONV2=A rail — so this branch
        // retires exactly as its previous note said it would. It used to ALLOW a
        // `frontend_rejected:` row for this stem as long as the reason was the
        // known E2402 `?`-conversion gap. There is no longer a known reason to
        // allow, so the allowance becomes the flat law: a shipped example that
        // the front end rejects is a defect, and naming it here is stronger than
        // the conditional it replaces.
        assert!(
            record.class != CorpusGateClass::FrontendRejected,
            "{stem} is frontend-rejected: {}. The E2402 `?`-conversion gap that \
             once excused this row is fixed (D-FAIL-CONV2=A), so a rejection here \
             is a live defect, not the known one",
            record.detail
        );
    }
}

/// c139 M3: string interpolation builds the same stdout as the interpreter.
#[test]
fn cranelift_covers_string_interpolation() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    n := 7\n    print(\"value {n}\")\n}\n",
        "str_interp",
    );
}

#[test]
fn cranelift_covers_shield_region() {
    let out = run_cranelift_without_fallback(
        "fn run() {\n    #Shield {\n        print(7)\n    }\n}\n",
        "shield_region",
    );
    assert_eq!(out.stdout, "7\n");
}

#[test]
fn cranelift_shield_defers_task_cancel_without_unwinding_native_frame() {
    // Prefer resident JIT; silent deopt to the interpreter is still I9-legal
    // when a nested Shield/channel shape is outside the resident subset.
    // `ch`/`ack_sender` cross into the task as bare captures, so they must be
    // `::` bindings: a `:=` binding is a changeable alias the owner could still
    // write, and D-CONC-FREEZE1=A refuses that crossing with E1101 before the
    // program ever runs. `::` is what every `examples/features/concurrency`
    // channel fixture uses; `^`/`freeze` are for values that really are owned
    // away or snapshotted, which is not what this test is about.
    with_jit_test_scope(|| {
        let out = run_cranelift_outcome(
            r#"use core.tasks as tasks
fn run() {
    (sender, ch) :: channel<Int>()
    (ack_sender, ack) :: channel<Int>()
    slow :: task {
               #Shield {
                   value :: ch.receive() ?? panic("closed")
                   print(value)
                   ack_sender.send(1)
               }
               print(99)
       }
    slow.cancel()
    sender.send(42)
    ack.receive() ?? panic("closed")
}
"#,
            "shield_cancel",
        );
        assert_eq!(out.stdout, "42\n");
    });
}

#[test]
fn cranelift_unshielded_receive_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
fn run() {
    (ready_sender, ready) :: channel<Int>()
    (sender, ch) :: channel<Int>()
    slow :: task {
        ready_sender.send(1)
        ch.receive() ?? panic("closed")
        print(99)
    }
    ready.receive() ?? panic("closed")
    slow.cancel()
    sender.send(42)
}
"#,
        "unshielded_receive_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_unshielded_sleep_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
use core.time as time
fn run() {
    (ready_sender, ready) :: channel<Int>()
    slow :: task {
        ready_sender.send(1)
        time.sleep(200ms)
        print(99)
    }
    ready.receive() ?? panic("closed")
    slow.cancel()
}
"#,
        "unshielded_sleep_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_unshielded_select_cancel_does_not_unwind_native_frame() {
    let out = run_cranelift_without_fallback(
        r#"use core.tasks as tasks
fn select_cancel_worker(ready_sender: Sender<Int>) {
    task.group worker {
        (_sender, ch) :: channel<Int>()
        ready_sender.send(1)
        if {
            value, ch :> print(value)
        }
        print(99)
    }
}

fn run() {
    task.group g {
    (ready_sender, ready) :: channel<Int>()
        slow :: task select_cancel_worker(ready_sender)
        ready.receive() ?? panic("closed")
        slow.cancel()
    }
}
"#,
        "unshielded_select_cancel",
    );
    assert_eq!(out.stdout, "");
}

#[test]
fn cranelift_wait_failures_recover_as_typed_task_failures() {
    let join_cancelled = r#"use core.time as time
fn failure_label(error: TaskFailure) => String {
    if error == {
        .Cancelled -> { return "cancelled" }
        .DeadlineBlown -> { return "deadline" }
        .Panicked(_) -> { return "panicked" }
    }
}
fn run() {
    child :: task {
        time.sleep(200ms)
    }
    child.cancel()
    result :: child.join()
    if result == {
        .Err(error) -> { print(failure_label(error)) }
        .Ok(_) -> { print("wrong cancellation") }
    }
}
"#;
    let RunOutcome::Ran {
        stdout,
        stderr,
        exit_code,
    } = run_cranelift_outcome_without_fallback(join_cancelled, "join_cancelled")
    else {
        panic!("joining a cancelled task must recover as TaskFailure.Cancelled")
    };
    assert_eq!(stdout, "cancelled\n");
    assert!(stderr.is_empty());
    assert_eq!(exit_code, 0);
}

/// #1486 / I9: task-group rich panic under default jet run must match AOT golden
/// stderr (full panic block + typed TaskFailure panic), including after a warm
/// re-invoke that reinstalls compile-time string handles.
#[test]
fn all_failfast_jit_stderr_matches_aot_golden() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let file = "examples/features/concurrency/all_failfast.jet";
    let expected = fs::read_to_string("examples/features/expected/concurrency/all_failfast.err.out")
        .expect("all_failfast.err.out");
    // Two invokes: first compiles; second proves reset_run_heap still resolves
    // rich-panic string handles on the same resident module.
    for _ in 0..2 {
        let got = match dev_iteration(file, false, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(ds) => panic!("all_failfast must Ran via JIT, got: {ds:?}"),
        };
        assert_eq!(got.exit_code, 70, "exit");
        assert_eq!(got.stderr, expected, "stderr");
        assert!(got.stdout.is_empty(), "stdout");
    }
}

/// c139 M3: checked integer arithmetic with overflow traps.
#[test]
fn cranelift_covers_checked_arithmetic() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    a := 10\n    b := 3\n    print(a + b)\n    print(a * b)\n    print(a - b)\n}\n",
        "arith",
    );
}

/// c139 M3: `let` chains and plain `if`/`else`.
#[test]
fn cranelift_covers_let_and_if() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    n := 5\n    if n > 3 {\n        print(1)\n    } else {\n        print(0)\n    }\n    m := n + 1\n    print(m)\n}\n",
        "let_if",
    );
}

/// c139 M3: calls between JIT-covered helper functions.
#[test]
fn cranelift_covers_function_calls() {
    assert_cranelift_matches_interpreter(
        "fn double(n: Int) => Int {\n    return n * 2\n}\nfn run() {\n    print(double(3))\n    print(double(0))\n}\n",
        "calls",
    );
}

#[test]
fn multi_head_functions_match_interpreter_resident_jit_and_aot() {
    require_multi_head_parity_prereqs();
    let src = "\
enum Shape {
    Circle(Float)
    Rect(left_1: Float, right_1: Float)
}
fn area(Circle(r: Float)) => Float { return r * r }
fn area(Rect(left_1: Float, right_1: Float)) => Float { return left_1 * right_1 }
fn run() {
    print(area(Shape.Circle(3.0)))
    print(area(.Rect.{ left_1: 2.0, right_1: 4.0 }))
}
";
    let dir = std::env::temp_dir().join(format!("jet_multi_head_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("multi_head.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().to_string();

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran { stdout, stderr, exit_code } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("interpreter failed multi-head functions: {diags:?}"),
    };
    let jit = run_cranelift_resident(src, "multi_head_functions");
    let default_dev = run_default_dev_resident(&shown, "multi_head_default_dev");
    let aot = compiled_binary_output(
        &dir,
        "multi_head_functions",
        0,
        "multi_head_functions",
        &shown,
    );
    let cli_run = run_cli_default_resident("run", &shown, "multi_head_cli_run");
    let cli_dev = run_cli_default_resident("dev", &shown, "multi_head_cli_dev");
    assert_eq!(default_dev, interpreted, "default dev/JIT drifted from interpreter");
    assert_eq!(jit, interpreted, "resident JIT drifted from interpreter");
    assert_eq!(aot, interpreted, "AOT drifted from interpreter");
    assert_eq!(cli_run, interpreted, "default `jet run` drifted from interpreter");
    assert_eq!(cli_dev, interpreted, "default `jet dev` drifted from interpreter");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn value_position_enum_patterns_match_all_execution_tiers() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    for stem in [
        "tooling/branch_dispatch_bench",
        "types/enum_dot",
        "types/enum_dot_patterns",
        "types/enums",
    ] {
        let file = example_path(stem);
        let expected = golden_program_output(stem);
        let interpreted = match dev_iteration(&file, false, true) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("{stem} must run in the TIR interpreter: {diags:?}")
            }
        };
        let default_dev = run_default_dev_resident(&file, &format!("{stem}_default"));
        let cli_run = run_cli_default_resident("run", &file, &format!("{stem}_cli"));
        let dir = common::unique_tmp(&format!("jet_{stem}_aot"));
        let aot = compiled_binary_output(&dir, "value_pattern", 0, stem, &file);

        assert_eq!(interpreted, expected, "interpreter drifted for {stem}");
        assert_eq!(default_dev, expected, "default resident JIT drifted for {stem}");
        assert_eq!(cli_run, expected, "default `jet run` drifted for {stem}");
        assert_eq!(aot, expected, "AOT drifted for {stem}");
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn multi_head_payload_range_checks_each_slot_across_runtime_tiers() {
    require_multi_head_parity_prereqs();
    let src = r#"enum Pair {
    Values(left: Int, right: Int)
    Empty
}
fn classify(pair: Pair) => String {
    if pair == {
        .Values(_, 10..19) -> { return "range" }
        .Values(_, _) -> { return "other" }
        .Empty -> { return "empty" }
    }
    return "unknown"
}
fn run() {
    print(classify(.Values.{ left: 1, right: 15 }))
    print(classify(.Values.{ left: 1, right: 25 }))
}
"#;
    let dir = common::unique_tmp("jet_multi_head_payload_range");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("multi_head_payload_range.jet");
    fs::write(&file, src).unwrap();
    let shown = file.to_string_lossy().into_owned();

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("interpreter failed payload-range fixture: {diags:?}"),
    };
    let jit = run_cranelift_resident(src, "multi_head_payload_range");
    let default_dev = run_default_dev_resident(&shown, "multi_head_payload_range_default_dev");
    let aot = compiled_binary_output(
        &dir,
        "multi_head_payload_range",
        0,
        "multi_head_payload_range",
        &shown,
    );
    let expected = ProgramOutput::ran("range\nother\n".into(), String::new(), 0);
    assert_eq!(interpreted, expected, "interpreter payload-range slot drifted");
    assert_eq!(jit, expected, "resident JIT payload-range slot drifted");
    assert_eq!(default_dev, expected, "default dev/JIT payload-range slot drifted");
    assert_eq!(aot, expected, "AOT payload-range slot drifted");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn multi_head_missing_head_e0307_reaches_all_diagnostic_entries() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui/multi_head_not_exhaustive.jet");
    let shown = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let snapshot = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui/multi_head_not_exhaustive.stderr"),
    )
    .unwrap();
    const SNAPSHOT_FILE: &str = "tests/ui/multi_head_not_exhaustive.jet";

    require_multi_head_parity_prereqs();
    let entries = multi_head_diagnostic_entries(&shown, &src);
    let expected = diagnostic_shapes(&entries.sema);
    assert_eq!(
        expected.len(),
        1,
        "missing-head fixture must produce exactly one diagnostic: {expected:?}"
    );
    assert_eq!(expected[0].code, "E0307");
    assert_eq!(
        jet::render_diagnostics(SNAPSHOT_FILE, &src, &entries.sema),
        snapshot,
        "the sema diagnostic must match the checked-in UI snapshot"
    );
    for command in ["run", "dev"] {
        assert_cli_diagnostic_snapshot(command, SNAPSHOT_FILE, &snapshot);
    }

    for (tier, diags) in [
        ("sema", RunOutcome::Problems(entries.sema)),
        ("AOT", RunOutcome::Problems(entries.aot)),
        ("resident JIT", entries.jit),
        ("default dev", entries.default_dev),
        ("forced interpreter diagnostic gate", entries.interpreter_gate),
    ] {
        let RunOutcome::Problems(diags) = diags else {
            panic!("{tier} entry must reject missing multi-head coverage")
        };
        assert_eq!(
            diagnostic_shapes(&diags),
            expected,
            "{tier} diagnostic code/text/span drifted from sema"
        );
        assert_eq!(
            jet::render_diagnostics(SNAPSHOT_FILE, &src, &diags),
            snapshot,
            "{tier} rendered diagnostic drifted from the UI snapshot"
        );
    }
    assert_eq!(
        entries.forced_interpreter,
        ProgramOutput::ran("first\n".into(), String::new(), 0),
        "successful forced interpreter entry must reach InterpreterBackend and keep first-head order"
    );
}

#[test]
fn multi_head_duplicate_head_l0301_keeps_first_match_across_runtime_tiers() {
    require_multi_head_parity_prereqs();
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_lint/multi_head_unreachable.jet");
    let shown = file.to_string_lossy().into_owned();
    let src = fs::read_to_string(&file).unwrap();
    let dir = common::unique_tmp("jet_multi_head_duplicate_head");
    fs::create_dir_all(&dir).unwrap();

    let mut bundle = jet::Loader::load_entry(&shown).expect("duplicate-head fixture should load");
    let sema = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        sema.iter().any(|diagnostic| diagnostic.code == "L0301"),
        "sema must retain duplicate-head lint: {sema:?}"
    );
    let compiled = jet::compile_with_path(&src, &shown)
        .expect("duplicate-head lint must not block AOT compilation");
    assert!(
        compiled
            .lints
            .iter()
            .any(|diagnostic| diagnostic.code == "L0301"),
        "AOT entry lost duplicate-head lint: {:?}",
        compiled.lints
    );

    let interpreted = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("interpreter rejected duplicate-head fixture: {diags:?}"),
    };
    let jit = run_cranelift_resident(&src, "multi_head_duplicate_head");
    let default_dev = run_default_dev_resident(&shown, "multi_head_duplicate_head_default_dev");
    let aot = compiled_binary_output(
        &dir,
        "multi_head_duplicate_head",
        0,
        "multi_head",
        &shown,
    );
    let cli_run = run_cli_default_resident("run", &shown, "multi_head_duplicate_head_cli_run");
    let cli_dev = run_cli_default_resident("dev", &shown, "multi_head_duplicate_head_cli_dev");
    let expected = ProgramOutput::ran("first\n".into(), String::new(), 0);
    assert_eq!(interpreted, expected, "interpreter first-head order drifted");
    assert_eq!(jit, expected, "resident JIT first-head order drifted");
    assert_eq!(default_dev, expected, "default dev/JIT first-head order drifted");
    assert_eq!(aot, expected, "AOT first-head order drifted");
    assert_eq!(cli_run, expected, "default `jet run` first-head order drifted");
    assert_eq!(cli_dev, expected, "default `jet dev` first-head order drifted");
    let _ = fs::remove_dir_all(&dir);
}

/// The three plain parameter modes across a call boundary: `read` (by value),
/// `&` (write-back), and `^` (take).
///
/// This was an `assert_cranelift_deopts_on_gap` case from the era when the
/// resident tier had no user-call write-back at all. It has one now — the
/// interpreter-side boundary right below this test only stays honest because
/// resolved user calls DO carry write-back — so the tier compiles this whole
/// body natively and correctly declines to deopt. The assertion follows the
/// coverage.
///
/// The tier assertions are the point, not decoration (the `float_lists`
/// lesson): a re-opened write-back gap is silent, because the deopt still
/// prints the right answer. `jit_executed_for_test` proves the resident engine
/// ran it, the deopt/fallback pair proves no arm went back to the interpreter,
/// and the helper compares stdout against the pure-interpreter baseline, so a
/// write-back that lands in the wrong slot fails too.
#[test]
fn cranelift_matches_plain_parameter_read_write_and_take_modes() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    // The interpreter baseline inside the helper runs on `InterpreterBackend`,
    // which never touches these thread-local flags, so resetting here is safe.
    jet_jit::reset_jit_trace_for_test();
    assert_cranelift_matches_interpreter(
        "fn read(text: String) { print(text) }\nfn edit(values: &[Int]) { values[0] = 9 }\nfn consume(text: ^String) { print(text) }\nfn run() {\n    text :: \"hello\"\n    values := [1, 2]\n    read(text)\n    edit(&values)\n    print(values[0])\n    consume(^text)\n}\n",
        "plain_parameter_modes",
    );
    assert!(
        jet_jit::jit_executed_for_test(),
        "plain_parameter_modes must reach the resident Cranelift tier"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "plain_parameter_modes must stay native: a deopt here means read/&/^ \
         parameter passing left the resident tier"
    );
}

#[test]
fn interpreter_writeback_boundary_only_opens_for_resolved_user_functions() {
    let user = bundle_of(
        "fn edit(value: &Int) { value = 2 }\nfn run() { value := 1; edit(&value); print(value) }\n",
        "user_writeback_boundary",
    );
    assert!(
        jet_driver::InterpreterBoundary::dev_boundary_scan(&user).is_none(),
        "resolved user calls have interpreter writeback support"
    );

    let unresolved = bundle_of(
        "fn run() { value := 1; unsupported(&value) }\n",
        "unsupported_writeback_boundary",
    );
    let boundary = jet_driver::InterpreterBoundary::dev_boundary_scan(&unresolved)
        .expect("an unresolved/core/import-style direct call must keep the honest boundary");
    assert_eq!(boundary.code, "E2201");
    assert!(boundary.what.contains("writeback"), "{boundary:?}");

    let mut foreign = bundle_of(
        "fn edit(value: &Int) {}\nfn run() { value := 1; edit(&value) }\n",
        "foreign_writeback_boundary",
    );
    let edit = foreign.modules[0]
        .items
        .iter_mut()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "edit" => Some(function),
            _ => None,
        })
        .expect("fixture has edit");
    let span = edit.name_span;
    edit.inline_foreign = Some(jet::AST::InlineForeign {
        lang: "c".to_string(),
        lang_span: span,
        marker_span: span,
        source: String::new(),
        source_span: span,
    });
    let boundary = jet_driver::InterpreterBoundary::dev_boundary_scan(&foreign)
        .expect("inline foreign functions are not interpreter writeback targets");
    assert_eq!(boundary.code, "E2201");
    assert!(boundary.what.contains("writeback"), "{boundary:?}");
}

/// A fixed `&` write-back parameter beside a variadic pack, read inside the
/// callee (`extras.len()`).
///
/// Same stale premise as `cranelift_matches_plain_parameter_read_write_and_take_modes`
/// above: this was an `assert_cranelift_deopts_on_gap` case from the era with
/// no resident user-call write-back, and the tier now compiles the whole body
/// natively, so it correctly declines to deopt. Native execution plus stdout
/// equality against the pure interpreter is strictly stronger than the old
/// deopt check, which never compared output at all: it catches both a
/// re-opened gap and a variadic count that arrives wrong in the callee.
#[test]
fn cranelift_matches_variadic_fixed_writeback() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    jet_jit::reset_jit_trace_for_test();
    assert_cranelift_matches_interpreter(
        "fn edit(values: &[Int], extras: ...Int) { values[0] = extras.len() }\nfn run() {\n    values := [0]\n    edit(&values, 7, 8)\n    print(values[0])\n}\n",
        "variadic_fixed_writeback",
    );
    assert!(
        jet_jit::jit_executed_for_test(),
        "variadic_fixed_writeback must reach the resident Cranelift tier"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "variadic_fixed_writeback must stay native: a deopt here means variadic \
         calls with a write-back parameter left the resident tier"
    );
}

/// c139 M3+: counted `loop init, cond, step` with compound assign.
#[test]
fn cranelift_covers_counted_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    sum := 0\n    loop i, 0..<5 {\n        sum += i\n    }\n    print(sum)\n}\n",
        "counted_loop",
    );
}

/// c139 M3+: `loop cond` while-form and compound assign.
#[test]
fn cranelift_covers_while_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    fuel := 3\n    loop fuel > 0 {\n        print(fuel)\n        fuel -= 1\n    }\n}\n",
        "while_loop",
    );
}

/// c139 M3+: inclusive range loop.
#[test]
fn cranelift_covers_range_loop() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    loop n, 1..3 {\n        print(n)\n    }\n}\n",
        "range_loop",
    );
}

/// c139 M3+: short-circuit && / ||.
#[test]
fn cranelift_covers_logic_ops() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    a := true\n    b := false\n    if a && !b {\n        print(1)\n    }\n    if b || a {\n        print(2)\n    }\n}\n",
        "logic_ops",
    );
}

/// c139 M3: string literals and locals passed to `print`.
#[test]
fn cranelift_covers_string_print() {
    assert_cranelift_matches_interpreter(
        "fn run() {\n    msg := \"hello, jit\"\n    print(msg)\n    print(\"done\")\n}\n",
        "strings",
    );
}

/// c125 Phase 2 / #1995: Float list values use the shared JetArena list path.
///
/// This was an `assert_cranelift_deopts_on_gap` case for as long as `xs[1..2]`
/// on a `[Float]` reached the int-only `jet_jit_list_slice`, which cloned
/// through `clone_int_list` and `.expect`ed inside its own `extern "C"` frame
/// — aborting the process. 9623985d0 made the window element-kind preserving,
/// which CLOSED that gap: the tier now compiles this whole body natively, so
/// it correctly declines to deopt. The assertion follows the coverage.
///
/// The tier assertions are the point, not decoration. This defect class is
/// silent — Cranelift declines a function, the tier deopts, and the program
/// still prints the right answer — so an output-only check would pass again
/// the moment the gap re-opened. `jit_executed_for_test` proves the resident
/// engine ran it, and the deopt/fallback pair proves no arm of it went back
/// to the interpreter.
#[test]
fn cranelift_covers_float_lists() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    // The interpreter baseline inside the helper runs on `InterpreterBackend`,
    // which never touches these thread-local flags, so resetting here is safe.
    jet_jit::reset_jit_trace_for_test();
    assert_cranelift_matches_interpreter(
        "fn run() {\n    xs := [Float].{ 1.5, 2.5 }\n    xs.push(3.5)\n    print(xs.len())\n    print(xs[0])\n    xs[1] = 4.5\n    print(xs[1])\n    mid :: xs[1..2]\n    print(mid[0])\n}\n",
        "float_lists",
    );
    assert!(
        jet_jit::jit_executed_for_test(),
        "float_lists must reach the resident Cranelift tier"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "float_lists must stay native: a deopt here means the float list gap re-opened"
    );
}

/// c125 Phase 2: records keep mixed scalar/String fields in JetArena.
#[test]
fn cranelift_covers_mixed_record_fields() {
    assert_cranelift_matches_interpreter(
        "struct Card {\n    name: String\n    score: Float\n    ready: Bool\n    mark: Char\n}\nfn run() {\n    c :: Card.{name: \"jet\", score: 2.5, ready: true, mark: 'J'}\n    print(c.name)\n    print(c.score)\n    print(c.ready)\n    print(c.mark)\n}\n",
        "mixed_record_fields",
    );
}

/// c139 M2: type-stable hot_swap re-links in the resident JIT and preserves
/// live runtime state; restart tears it down.
#[test]
fn cranelift_hot_swap_preserves_live_state() {
    // One session, one thread. The resident module, the live heap and the
    // `#Persist` store are scoped to the thread, not to the call or to the
    // backend value (D-HOTSWAP1 / D-PERSIST1) -- `backend2` below re-links the
    // very session `backend` started. So the sized stack goes around the whole
    // session; `CraneliftBackend::run`'s own boundary then runs inline.
    with_jit_test_scope(cranelift_hot_swap_preserves_live_state_inner);
}

/// c125 P0 regression guard: a runtime stop under the default JIT backend
/// (list index OOB here; the same trapped-flag mechanism covers checked-arith
/// overflow and the two concurrency panic sites) must report cleanly — not
/// kill the resident process. The next hot-reload iteration in the SAME
/// process must then run cleanly, proving the trap didn't leak into the next
/// run's heap and the process is still alive to serve it. Before the fix,
/// every one of these host shims called `std::process::exit(70)` directly,
/// which took the whole `jet dev` server down with it.
///
/// The reported shape is `Ran { exit_code: 70 }` carrying the registered
/// `Stop [E3010]`, NOT `Problems`. #1483 (0a291f5bb) retired the old
/// `Problems(E0953)` route precisely because a live-program trap is not a
/// comptime build failure; `jit_1216_adversarial_regressions` and
/// `uninit_fixed_dynamic_oob_uses_the_resident_jit_trap_path` pin the same
/// contract for the same OOB shape, and AOT `jet_panic` agrees (I9).
#[test]
fn cranelift_trap_then_hot_swap_continues() {
    // Session-scoped boundary; see `cranelift_hot_swap_preserves_live_state`.
    with_jit_test_scope(cranelift_trap_then_hot_swap_continues_inner);
}

/// The dev iteration surfaces front-end errors identically to batch
/// compilation (D-DEV: same diagnostics).
#[test]
fn front_end_errors_surface_in_dev_iteration() {
    // Write a broken program to a temp file.
    let dir = std::env::temp_dir();
    let file = dir.join("jet_dev_broken.jet");
    fs::write(&file, "fn run() {\n    print(nope);\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();
    match dev_iteration(&shown, false, true) {
        RunOutcome::Problems(diags) => {
            assert!(!diags.is_empty(), "broken program must report problems");
            assert!(
                diags
                    .iter()
                    .all(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
                "dev should surface errors"
            );
        }
        RunOutcome::Ran { .. } => panic!("a broken program must not run"),
    }
}

/// A body-only edit keeps the type surface stable → swap (Ok).
#[test]
fn body_only_edit_is_type_stable() {
    let old = bundle_of(STRUCT_OLD, "stable_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n}\nfn f(p: P) => Int {\n    return p.x + 1\n}\nfn run() {\n    print(f(P.{x: 2}))\n}\n",
        "stable_new",
    );
    assert!(
        jet::Sema::HotSwap::type_stable_check(&old, &new, "run").is_ok(),
        "a body-only edit must be type-stable (swap path)"
    );
}

/// Adding a struct field changes the surface → restart, with E2210 naming it.
#[test]
fn struct_field_change_emits_e2210() {
    let old = bundle_of(STRUCT_OLD, "field_old");
    let new = bundle_of(
        "struct P {\n    x: Int\n    y: Int\n}\nfn f(p: P) => Int {\n    return p.x\n}\nfn run() {\n    print(f(P.{x: 1, y: 2}))\n}\n",
        "field_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("adding a struct field must force a restart"),
        Err(diags) => {
            assert_eq!(diags.len(), 1);
            assert_eq!(diags[0].code, "E2210");
            assert!(
                diags[0].what.contains("struct `P`"),
                "E2210 should name the changed struct, got: {}",
                diags[0].what
            );
        }
    }
}

/// Changing a function's return type changes the surface → E2210.
#[test]
fn fn_signature_change_emits_e2210() {
    let old = bundle_of(
        "fn g(a: Int) => Int {\n    return a\n}\nfn run() {\n    print(g(1))\n}\n",
        "sig_old",
    );
    let new = bundle_of(
        "fn g(a: Int) => Bool {\n    return a == 0\n}\nfn run() {\n    print(g(1))\n}\n",
        "sig_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("a return-type change must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("return type"));
        }
    }
}

/// Adding an enum variant changes the surface → E2210.
#[test]
fn enum_variant_change_emits_e2210() {
    let old = bundle_of(
        "enum E {\n    A\n    B\n}\nfn run() {\n    print(1)\n}\n",
        "enum_old",
    );
    let new = bundle_of(
        "enum E {\n    A\n    B\n    C\n}\nfn run() {\n    print(1)\n}\n",
        "enum_new",
    );
    match jet::Sema::HotSwap::type_stable_check(&old, &new, "run") {
        Ok(()) => panic!("adding an enum variant must force a restart"),
        Err(diags) => {
            assert_eq!(diags[0].code, "E2210");
            assert!(diags[0].what.contains("enum `E`"));
        }
    }
}

/// D-PERSIST-DEVSTATE1=A: release codegen lowers a writable `#Persist` binding
/// to the safe interior-mutable Prelude cell. The storage mechanism is shared
/// with dev-tier reload state; generated Rust only marshals reads and writes
/// through that cell.
#[test]
fn persist_binding_codegen_uses_safe_prelude_cell() {
    let dir = std::env::temp_dir().join(format!("jet_persist_parity_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counter.jet");
    let src = "#Persist counter := 0\nfn run() {\n    counter += 1\n    print(counter)\n}\n";
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().to_string();
    let rust = jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected fixture:\n{}",
                jet::render_diagnostics(&shown, src, &diags)
            )
        })
        .rust;
    // The binding under test is `counter`. It reaches Rust through the single
    // naming law: `mangle("counter")` prefixes `GENERATED_NAME_PREFIX` (`__jet_`,
    // crates/jet-foundation/src/Syntax/predicates.rs:367) and `emit_const`
    // uppercases the result, so the generated slot is `__JET_COUNTER`. Name the
    // binding in the message because the dump also contains unconditional
    // harness statics.
    assert!(
        rust.contains("static __JET_COUNTER: JetPersistCell<i64> = JetPersistCell::new(0i64);"),
        "`#Persist counter := 0` must lower to a Prelude `JetPersistCell`:\n{rust}"
    );
    assert!(
        rust.contains("(__JET_COUNTER).set(jet_std::jet_int_add((__JET_COUNTER).get(), 1i64));"),
        "writes to `#Persist counter` must use the Prelude cell:\n{rust}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// D-PERSIST1: `#Persist` module bindings survive a real hot reload when the
/// shape is compatible; an incompatible shape reset reports the exact reason
/// and reseeds from the new initializer. Shared store is consulted by both
/// Cranelift and interpreter tiers.
#[test]
fn persist_binding_survives_hot_swap_and_resets_on_shape_change() {
    // `jet_foundation::Persist` is a thread-local store, so run/hot_swap/
    // restart must share one thread; see
    // `cranelift_hot_swap_preserves_live_state`.
    with_jit_test_scope(persist_binding_survives_hot_swap_and_resets_on_shape_change_inner);
}

// ── card #131 S1-bridge (D-SERDE2): hand codec dev-tier parity (R12) ──────────
// A hand `impl T.Encode`/`impl T.Decode` uses the same typed TIR codec dispatch
// as a derived codec. The dev tier must execute the round trip, not preserve the
// retired E2201 coverage gap.
#[test]
fn hand_written_codec_dev_tier_matches_native_shape() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    const SRC: &str = r#"
use core.encoding.json as json

struct Email { addr: String }

impl Email.Encode {
    fn encode(self) => DataTree {
        m :: [String:DataTree].{ "email": DataTree.Text(~self.addr) }
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) => Email ? [FieldError] {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return Ok(Email.{addr: s})
    }
}

fn run() {
    e := Email.{addr: "a@b.com"}
    s := json.to_string(e)
    print(s)
    back := json.decode<Email>(s) ?? panic("decode failed")
    print(back.addr)
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_hand_codec_dev_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hand_codec.jet");
    fs::write(&file, SRC).unwrap();
    let outcome = dev_iteration_with_timeout("hand_codec", file.to_str().unwrap(), true);
    match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(stdout, "{\"email\":\"a@b.com\"}\na@b.com\n");
            assert!(stderr.is_empty(), "hand codec wrote stderr: {stderr}");
            assert_eq!(exit_code, 0);
        }
        RunOutcome::Problems(diags) => panic!("hand codec interpreter failed: {diags:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): `jet dev`'s due-job tick
/// consumer. `scheduled_jobs` must enumerate every `#Job #Every(…)` fn
/// with its resolved schedule (and skip a plain `#Job fn` with no
/// `#Every(…)`), and `run_named_job` must actually execute one by name
/// through the same interpreter tier `dev_iteration` uses — golden-testing
/// the loop's per-tick logic without the long-running file watcher, same
/// spirit as `dev_iteration` itself (see the module doc above).
#[test]
fn schedule_every_dev_loop_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let file = root.join("examples/features/devloop/schedule_every.jet");
    let src = fs::read_to_string(&file).unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap())
        .unwrap_or_else(|diags| panic!("schedule_every.jet failed to load: {diags:?}"));
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "schedule_every.jet must compile clean:\n{}",
        jet::render_diagnostics("schedule_every.jet", &src, &diags)
    );

    let mut jobs = jet::Interpreter::scheduled_jobs(&bundle);
    jobs.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = jobs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "compact_archive",
            "nightly_backup",
            "prune_sessions",
            "refresh_indexes",
        ],
        "scheduled_jobs must list every #Job fn carrying #Every(…), and skip the \
         #Every(…)-less `manual_only` job"
    );
    let schedules: std::collections::HashMap<&str, &jet::AST::EverySchedule> =
        jobs.iter().map(|(n, s)| (n.as_str(), s)).collect();
    assert_eq!(
        *schedules["prune_sessions"],
        jet::AST::EverySchedule::Duration {
            nanos: 5 * 60 * 1_000_000_000
        },
        "`#Every(5min)` must resolve to a 5-minute interval"
    );
    assert_eq!(
        *schedules["nightly_backup"],
        jet::AST::EverySchedule::WallClockTime { hour: 3, minute: 0 },
        "`#Every(\"03:00\")` must resolve to 03:00 daily"
    );
    assert_eq!(
        *schedules["refresh_indexes"],
        jet::AST::EverySchedule::Duration {
            nanos: 2 * 60 * 60 * 1_000_000_000
        },
        "`#Every(2h)` must resolve through the canonical Time family"
    );
    assert_eq!(
        *schedules["compact_archive"],
        jet::AST::EverySchedule::Duration {
            nanos: 24 * 60 * 60 * 1_000_000_000
        },
        "`#Every(1d)` must resolve through the canonical Time family"
    );

    // Actually invoking a named job runs it like an ordinary call.
    match jet::Interpreter::run_named_job(&bundle, "prune_sessions", false) {
        RunOutcome::Ran { stdout, exit_code, .. } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "pruning sessions\n");
        }
        RunOutcome::Problems(diags) => panic!("run_named_job failed: {diags:?}"),
    }
}

/// c728 C6: a watching `jet dev` session deopts on a JIT-gap edit and accepts a
/// later valid edit; one-shot dev exits 0.
#[test]
fn watching_dev_reruns_on_jit_gap_and_recovers() {
    let dir = std::env::temp_dir().join(format!("jet_dev_watch_c6_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.jet");
    fs::write(&file, "fn run() {\n    print(\"good-v1\")\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"));
    child
        .arg("dev")
        .arg(&shown)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("spawn watching jet dev");

    std::thread::sleep(Duration::from_millis(800));
    fs::write(
        &file,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    fs::write(&file, "fn run() {\n    print(\"good-v2\")\n}\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));

    let _ = child.kill();
    let out = child.wait_with_output().expect("watching jet dev output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("good-v1"), "stdout:\n{stdout}");
    assert!(
        !stderr.contains("E2211") && !stdout.contains("E2211"),
        "retired E2211 must not appear\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("good-v2"), "stdout:\n{stdout}");

    fs::write(
        &file,
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let once = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", &shown, "--watch=off"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let once_stdout = String::from_utf8_lossy(&once.stdout);
    let once_stderr = String::from_utf8_lossy(&once.stderr);
    assert_eq!(once.status.code(), Some(0), "stderr={once_stderr}");
    assert!(
        !once_stderr.contains("E2211") && !once_stdout.contains("E2211"),
        "retired E2211 must not appear: stdout={once_stdout} stderr={once_stderr}"
    );
    assert!(
        !once_stdout.trim().is_empty(),
        "deopted one-shot dev should print output: stdout={once_stdout} stderr={once_stderr}"
    );
}

/// #439 / E3-UL6: native matrix — dependency-aware WatchSession invalidates
/// the exact closure, meets edit-to-visible budget, and recovers after a
/// simulated crash/reconnect. AOT rebuild semantics match the prior run.
#[test]
fn ul6_native_watch_matrix_budget_and_reconnect() {
    let dir = std::env::temp_dir().join(format!(
        "jet_ul6_native_{}_{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("app.jet");
    let lib = dir.join("lib.jet");
    let asset = dir.join("app.css");
    fs::write(&lib, "// helper module\n").unwrap();
    fs::write(&asset, "body{}\n").unwrap();
    fs::write(&entry, "fn run() {\n    print(\"v1\")\n}\n").unwrap();

    let mut graph = jet_devserver::WatchGraph::from_entry(&entry, &[lib.clone()]).unwrap();
    graph.upsert(asset.clone(), jet_devserver::RootKind::Style);
    graph.link(
        std::fs::canonicalize(&entry).unwrap_or(entry.clone()),
        asset.clone(),
    );
    let mut session = jet_devserver::WatchSession::from_graph(graph);
    assert!(session.graph().node_count() >= 3);
    let kinds: std::collections::BTreeSet<_> =
        session.graph().nodes().map(|n| n.kind).collect();
    assert!(kinds.contains(&jet_devserver::RootKind::Import));
    assert!(kinds.contains(&jet_devserver::RootKind::Style));

    std::thread::sleep(Duration::from_millis(30));
    fs::write(&lib, "// helper module v2\n").unwrap();
    let started = Instant::now();
    let receipt = session.poll().expect("lib invalidation");
    let visible_ms = started.elapsed().as_millis();
    assert!(
        receipt
            .closure
            .iter()
            .any(|p| p.ends_with("lib.jet") || p.ends_with("app.jet")),
        "closure={:?}",
        receipt.closure
    );
    assert!(
        visible_ms <= jet_devserver::EDIT_TO_VISIBLE_BUDGET_MS
            || jet_devserver::within_budget(&receipt),
        "edit-to-visible {visible_ms}ms receipt={:?}",
        receipt.edit_to_visible_ms
    );
    assert!(receipt.render().contains("\"generation\":"));
    session.acknowledge(&receipt).unwrap();

    // Crash/reconnect: recover stamps, then a fresh edit still fires once.
    std::thread::sleep(Duration::from_millis(30));
    fs::write(&lib, "// helper module v3\n").unwrap();
    session.recover();
    assert!(session.poll().is_none(), "recover must clear pending drift");
    std::thread::sleep(Duration::from_millis(30));
    fs::write(&entry, "fn run() {\n    print(\"v4\")\n}\n").unwrap();
    let again = session.poll().expect("post-reconnect edit");
    session.acknowledge(&again).unwrap();

    // AOT parity: one-shot run of the final source.
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", &entry.to_string_lossy()])
        .env("NO_COLOR", "1")
        .output()
        .expect("jet run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "v4");

    let _ = fs::remove_dir_all(&dir);
}

/// #439 / E3-UL6: `jet run --watch` and `jet dev` share WatchSession receipts.
#[test]
fn ul6_run_watch_and_dev_share_engine() {
    let dir = std::env::temp_dir().join(format!(
        "jet_ul6_share_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.jet");
    fs::write(&file, "fn run() {\n    print(\"v1\")\n}\n").unwrap();
    let shown = file.to_string_lossy().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", &shown, "--watch"])
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jet run --watch");

    std::thread::sleep(Duration::from_millis(900));
    fs::write(&file, "fn run() {\n    print(\"v2\")\n}\n").unwrap();
    std::thread::sleep(Duration::from_millis(900));
    let _ = child.kill();
    let out = child.wait_with_output().expect("run --watch output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("watching") || stdout.contains("v1") || stdout.contains("v2"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("v1") || stdout.contains("changed") || stdout.contains("v2"),
        "expected watch activity\nstdout:\n{stdout}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// #778 C1–C3: per-function tiers, cross-tier host-shim calls, and --trace-tiers.
///
/// The fixture needs one function the planner keeps native AND one it binds to
/// the canonical TIR interpreter; a program that is native end to end cannot
/// demonstrate per-function selection at all. This test used to get its split
/// from `[a, b] :: doubled`, which the resident subset did not cover. Commit
/// 3211b6944 ("fix(#729): JIT list destructure + is_empty") lowered list
/// destructure natively (`crates/jet-jit/src/jit/safety.rs`
/// `resident_safe_stmt`'s `TStmt::ListDestructure` arm, plus the matching
/// `lower_ctx.rs` lowering), so that split closed for the right reason and the
/// old "gap must be interpreter-bound" assertion went stale in the GOOD
/// direction. Reintroducing a lowering gap to keep it would be an I9 violation,
/// so the split now comes from the one that is architectural rather than a
/// missing lowering: D-MEMO1=A keeps the memo store in the Prelude and makes the
/// resident engine an adapter, so a `#Memo` function crosses the deopt boundary
/// by design (`crates/jet-jit/src/jit/tiers.rs` `plan_tiers`, and the
/// `DEOPT_MEMOS` carrier in `crates/jet-jit/src/jit/deopt.rs` that exists only
/// to serve this cross-tier call). `native_sum` keeps the destructure so the
/// closed #729 gap stays pinned as native coverage instead of being deleted.
#[test]
fn tiered_run_selects_per_function_tiers_and_cross_calls() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jet_778_tiers");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mixed.jet");
    fs::write(
        &file,
        r#"#Memo fn cached(n: Int) =[]=> Int :: n * 2

fn add1(n: Int) => Int {
    return n + 1
}

fn native_sum() => Int {
    doubled :: [add1(40), add1(1)]
    [a, b] :: doubled
    return a + b
}

fn run() {
    print(native_sum())
    print(cached(21) + add1(0))
}
"#,
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let bundle = checked_bundle_from_path(&shown);
    let plan = jet_jit::plan_bundle_tiers(&bundle);
    for name in ["run", "add1", "native_sum"] {
        assert!(
            plan.native.contains(name),
            "`{name}` must select the native tier; native={:?} deopt={:?} whole={}",
            plan.native,
            plan.deopt,
            plan.whole_interp
        );
    }
    assert!(
        !plan.whole_interp,
        "a per-function split must not collapse into whole-program interpretation; \
         native={:?} deopt={:?}",
        plan.native,
        plan.deopt
    );
    let interp_bound: Vec<&str> = plan.deopt.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        interp_bound,
        ["cached"],
        "only the memoized function is interpreter-bound; native={:?} deopt={:?}",
        plan.native,
        plan.deopt
    );
    assert!(
        plan.deopt
            .iter()
            .any(|(_, reason)| reason.contains("canonical Prelude cache")),
        "the deopt must name D-MEMO1=A's shared Prelude cache, not an incidental \
         lowering gap; deopt={:?}",
        plan.deopt
    );

    jet_jit::reset_jit_trace_for_test();
    jet_jit::set_trace_tiers(true);
    let mut backend = CraneliftBackend::new();
    let stdout = match backend.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(ds) => panic!("mixed/deopt program must run: {ds:?}"),
    };
    jet_jit::set_trace_tiers(false);
    assert_eq!(
        stdout.trim(),
        "43\n43",
        "mixed-tier run should print the native sum then the cross-tier sum, got {stdout:?}"
    );
    // Both tiers must actually have executed: the native entry, and the host
    // shim that re-entered the canonical interpreter for `cached`.
    assert!(
        jet_jit::jit_executed_for_test(),
        "the native tier must have run"
    );
    assert!(
        jet_jit::deopt_invoked_for_test(),
        "the cross-tier host shim must have entered the interpreter"
    );
    let trace = jet_jit::take_last_trace();
    assert!(!trace.is_empty(), "trace-tiers must record rows");
    assert!(
        trace.iter().any(|row| !row.function.is_empty()),
        "trace rows need function names: {trace:?}"
    );
    assert!(
        trace
            .iter()
            .any(|row| matches!(row.tier, jet_jit::Tier::Native) && row.function == "run"),
        "trace must record the native entry: {trace:?}"
    );
    assert!(
        trace.iter().any(|row| {
            matches!(row.tier, jet_jit::Tier::Interp)
                && row.function == "cached"
                && !row.reason.is_empty()
        }),
        "trace must name the interpreter-bound function and its reason: {trace:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tracked_float_origin_matches_aot_in_default_dev() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let dir = common::unique_tmp("jet_float_binding_origin_dev");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("float_binding_origin.jet");
    fs::write(
        &file,
        "fn run() {\n    #Track speed :: 3.5\n    plain :: 3.5\n    copied :: speed\n    print(speed.origin())\n    print((speed).origin())\n    print(plain.origin())\n    print(copied.origin())\n    print(next().origin())\n}\nfn next() => Float {\n    print(\"evaluated\")\n    return 3.5\n}\n",
    )
    .unwrap();
    let shown = file.to_string_lossy().to_string();
    let expected_stdout = format!(
        "tracked `speed` at {shown}:2:12: #Track speed :: 3.5\ntracked `speed` at {shown}:2:12: #Track speed :: 3.5\nuntracked\nuntracked\nevaluated\nuntracked\n"
    );
    let aot = compiled_binary_output(
        &dir,
        "float_binding_origin",
        0,
        "float_binding_origin",
        &shown,
    );
    let resident = run_cranelift_resident_file(&shown, "float_binding_origin");
    let dev = match dev_iteration_with_timeout("float_binding_origin", &shown, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("default dev failed Float origin: {diags:?}"),
    };

    assert_eq!(aot, ProgramOutput::ran(expected_stdout, String::new(), 0));
    assert_eq!(resident, aot);
    assert_eq!(dev, aot);

    let interpreted = match dev_iteration_with_timeout("float_binding_origin", &shown, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => panic!("forced interpreter failed Float origin: {diags:?}"),
    };
    assert_eq!(interpreted, aot);
    let _ = fs::remove_dir_all(&dir);
}

/// Tower #1754: keep the repaired collection surface on a small, explicit
/// three-way parity lens. The existing corpus battery is intentionally broad;
/// this test makes the card's four rows independently runnable.
#[test]
fn tower_1754_collection_parity_focus() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    with_jit_test_scope(|| {
        let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
        for stem in [
            "collections/iter_adapters",
            "collections/iter_tools_audit",
            "collections/list_surface",
            "effects/taint",
        ] {
            assert_cranelift_three_way(&example_path(stem), stem);
        }
    });
}

/// #2020: every `tests/dev_parts/*.rs` slice is wired into a real cargo test
/// target.
///
/// The suite was split because one binary could not run its own declared tests
/// inside the 900s guard, and a split has exactly one silent failure mode: a
/// slice that no `tests/dev*.rs` target `include!`s compiles nowhere and runs
/// nowhere, so its tests stop existing without anything going red. That is the
/// shape that let five `jit_run` failures sit unrecorded for a whole session —
/// the tests were not in a routine set and nothing said so.
///
/// Naming the target is all it takes to be routine: cargo makes a test binary
/// out of every `tests/*.rs` file by itself, `scripts/agent/time-suites.sh`
/// times each one against this same guard, and `tools/ci/test-shards.sh`
/// enumerates the whole target inventory fresh on every run (D-CI1=A). So the
/// only thing left to check is that no slice is orphaned.
#[test]
fn every_dev_slice_is_wired_into_a_test_target() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut wired = String::new();
    for entry in fs::read_dir(&tests_dir).expect("tests dir").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("dev") && name.ends_with(".rs") {
            wired.push_str(&fs::read_to_string(entry.path()).expect("dev target source"));
        }
    }
    let mut orphans = Vec::new();
    for entry in fs::read_dir(tests_dir.join("dev_parts"))
        .expect("dev_parts dir")
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".rs") && !wired.contains(&format!("dev_parts/{name}")) {
            orphans.push(name);
        }
    }
    orphans.sort();
    assert!(
        orphans.is_empty(),
        "no tests/dev*.rs target includes these slices, so every test in them runs \
         nowhere: {orphans:?}"
    );
}
