mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, build_and_run_full, have_rustc, run_default_multi};
use jet::Interpreter::{dev_iteration, RunOutcome};
use std::fs;

const OUTER_GROUP_HELPER: &str = r#"
fn spawn_later(group: TaskGroup) => Shared<[Int]> {
    gate :: Shared.new([0])
    task {
        gate.edit((state: [Int]) => state[0] = state[0] + 1)
        loop gate.read((state: [Int]) => state[0]) == 1 {}
        gate.edit((state: [Int]) => state[0] = state[0] + 1)
        total := 0
        loop n, 0..<2000000 { total += n }
        print("task")
    }
    loop gate.read((state: [Int]) => state[0]) == 0 {}
    return ~gate
}

fn run() {
    task.group group {
        gate :: spawn_later(group)
        print("inside")
        gate.edit((state: [Int]) => state[0] = state[0] + 1)
        loop gate.read((state: [Int]) => state[0]) < 3 {}
    }
    print("after")
}
"#;

fn error_codes(source: &str) -> Vec<String> {
    jet::compile(source)
        .expect_err("source must be rejected")
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn interpreter_outcome(name: &str, source: &str) -> RunOutcome {
    let path = std::env::temp_dir().join(format!(
        "jet_taskgroup_{name}_{}.jet",
        std::process::id()
    ));
    fs::write(&path, source).unwrap();
    dev_iteration(path.to_str().unwrap(), false, true)
}

fn assert_jit_compiles(name: &str, source: &str) {
    // Resident JIT lowering of nested task/lambda shapes needs more than the
    // default ~2MiB test-thread stack; keep the compile off the test thread.
    let name = name.to_string();
    let source = source.to_string();
    let join = std::thread::Builder::new()
        .name(format!("jet-taskgroup-jit-{name}"))
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let path = std::env::temp_dir().join(format!(
                "jet_taskgroup_jit_{name}_{}.jet",
                std::process::id()
            ));
            fs::write(&path, &source).unwrap();
            let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
            let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
                .into_iter()
                .filter(|diagnostic| {
                    matches!(
                        diagnostic.severity,
                        jet::Diagnostics::Severity::Error
                    )
                })
                .collect::<Vec<_>>();
            assert!(errors.is_empty(), "{errors:?}");
            jet_jit::try_compile_bundle(&bundle)
                .expect("TaskGroup source must compile for resident JIT");
        })
        .expect("spawn JIT compile thread");
    join.join().expect("JIT compile thread");
}

#[test]
fn named_function_parameters_spawn_copy_and_owned_captures() {
    if !have_rustc() {
        return;
    }
    // D-CONC-SPAWN1=D: the bare `task` keyword binds to the innermost active
    // group (`taskgroup_stack.last()`), so it cannot target a specific one of
    // two simultaneous `TaskGroup` parameters the way `first.task => …` /
    // `second.task => …` could under the retired spelling. The ratified text
    // is silent on multi-group-per-function, so this splits the old
    // `spawn_both(first, second)` into two single-group helpers instead of
    // guessing a new qualified-spawn syntax (see card #1854 log).
    let source = r#"
fn spawn_one(group: TaskGroup, value: Int) {
    handle :: task value + 1
    print(handle.join() ?? 0)
}

fn spawn_owned(group: TaskGroup, values: ^[Int]) {
    handle :: task values[0]
    print(handle.join() ?? 0)
}

fn spawn_both(group: TaskGroup) {
    left :: task 20
    right :: task 22
    print((left.join() ?? 0) + (right.join() ?? 0))
}

fn run() {
    task.group group {
        spawn_one(group, 41)
        values :: [7, 8, 9]
        spawn_owned(group, ^values)
        task.group inner {
            spawn_both(inner)
        }
    }
}
"#;
    let (code, stdout) = build_and_run("taskgroup_parameter", source);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n7\n42\n");
}

#[test]
fn lexical_group_joins_anonymous_helper_spawn() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("taskgroup_parameter_join", OUTER_GROUP_HELPER);
    assert_eq!(code, 0);
    assert_eq!(stdout, "inside\ntask\nafter\n");
}

#[test]
fn default_run_joins_helper_spawn_before_outer_exit() {
    match interpreter_outcome("parameter_join", OUTER_GROUP_HELPER) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "{stderr}");
            assert_eq!(stderr, "", "{stderr}");
            assert_eq!(stdout, "inside\ntask\nafter\n");
        }
        RunOutcome::Problems(diags) => panic!("interpreter rejected helper spawn: {diags:?}"),
    }
    assert_jit_compiles("parameter_join", OUTER_GROUP_HELPER);
    let (code, stdout, stderr) = run_default_multi(
        "taskgroup_parameter_join",
        "main.jet",
        &[("main.jet", OUTER_GROUP_HELPER)],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "inside\ntask\nafter\n", "{stderr}");
    assert!(
        stderr.lines().any(|line| {
            line.split_whitespace()
                .take(3)
                .eq(["run", "tier1", "native"])
        }),
        "{stderr}"
    );
}

#[test]
fn parameter_spawn_rejects_view_capture() {
    let source = r#"
fn spawn_view(group: TaskGroup, values: View<Int>) {
    handle :: task values[0]
    print(handle.join() ?? 0)
}

fn run() {}
"#;
    assert_eq!(error_codes(source), ["E1102"]);
}

#[test]
fn taskgroup_type_is_second_class() {
    for (source, expected) in [
        (
            "struct Bad { group: TaskGroup }\nfn run() {}\n",
            &["E1110"][..],
        ),
        (
            "fn bad() => TaskGroup { return 0 }\nfn run() {}\n",
            &["E0119", "E0113"][..],
        ),
        (
            "fn run() { group: TaskGroup :: 0 }\n",
            &["E0003"][..],
        ),
        (
            "fn run() { f :: (group: TaskGroup) => 0 }\n",
            &["E0119"][..],
        ),
        (
            "fn bad(group: TaskGroup) { alias :: group }\nfn run() {}\n",
            &["E0120"][..],
        ),
        (
            "fn bad(group: TaskGroup) { groups :: [group] }\nfn run() {}\n",
            &["E1110"][..],
        ),
        (
            "fn bad(group: TaskGroup) { pair :: (group: group, n: 1) }\nfn run() {}\n",
            &["E1110"][..],
        ),
        (
            "fn bad(group: TaskGroup) { maybe :: Val(group) }\nfn run() {}\n",
            &["E1110"][..],
        ),
    ] {
        assert_eq!(error_codes(source), expected, "{source}");
    }
}

#[test]
fn taskgroup_cannot_escape_in_a_closure() {
    let source = r#"
fn use_group(group: TaskGroup) => Int = 1

fn escape(group: TaskGroup) => fn() => Int {
    return () => use_group(group)
}

fn run() {}
"#;
    assert_eq!(error_codes(source), ["E1110"]);
}

#[test]
fn evaluator_supports_taskgroup_combinators() {
    let source = r#"
use core.time as time

fn slow_seven() => Int {
    time.sleep(30)
    return 7
}

fn slow_eleven() => Int {
    time.sleep(30)
    return 11
}

fn run() {
    task.group all_group {
        values :: (task.all { 1, 2 }) ?? panic("all failed")
        print(values[0] + values[1])
    }
    task.group race_group {
        print((task.race { slow_seven(), 8 }) ?? panic("race failed"))
    }
    task.group any_group {
        print((task.any { slow_eleven(), 12 }) ?? panic("any failed"))
    }
}
"#;
    let interpreted = match interpreter_outcome("combinators", source) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "{stderr}");
            stdout
        }
        RunOutcome::Problems(diags) => panic!("interpreter rejected task groups: {diags:?}"),
    };
    assert_eq!(interpreted, "3\n8\n12\n");

    let (code, stdout, stderr) =
        run_default_multi("taskgroup_combinators", "main.jet", &[("main.jet", source)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, interpreted, "{stderr}");

    if have_rustc() {
        let (code, stdout) = build_and_run("taskgroup_combinators", source);
        assert_eq!(code, 0);
        assert_eq!(stdout, interpreted);
    }
}

fn assert_native_wait_exit(
    name: &str,
    source: &str,
    stderr_text: &str,
    expected_stdout: &str,
) {
    assert_jit_compiles(name, source);
    let (code, stdout, stderr) = run_default_multi(name, "main.jet", &[("main.jet", source)]);
    assert_ne!(code, 0, "{stderr}");
    assert_eq!(stdout, expected_stdout, "{stderr}");
    assert!(!stdout.contains("caller"), "{stdout:?}\n{stderr}");
    assert!(stderr.contains(stderr_text), "{stderr}");
    assert!(
        stderr.lines().any(|line| {
            line.split_whitespace()
                .take(3)
                .eq(["run", "tier1", "native"])
        }),
        "{stderr}"
    );
}

fn assert_group_close_success(name: &str, source: &str, expected_stdout: &str) {
    let interpreted = match interpreter_outcome(name, source) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "interpreter: {stderr}");
            assert!(
                stderr
                    .lines()
                    .all(|line| line.contains("tier0 interp")),
                "interpreter diagnostics: {stderr}"
            );
            stdout
        }
        RunOutcome::Problems(diags) => panic!("interpreter rejected {name}: {diags:?}"),
    };
    assert_eq!(interpreted, expected_stdout, "interpreter output drifted");

    // Nested task/group lowering needs more than the default test-thread
    // stack. Keep the resident probe on the same 8 MiB stack as the focused
    // JIT compile helper above; this changes only probe capacity, not runtime
    // semantics.
    let probe_name = name.to_string();
    let probe_source = source.to_string();
    let probe = std::thread::Builder::new()
        .name(format!("jet-taskgroup-resident-{probe_name}"))
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let jit_path = std::env::temp_dir().join(format!(
                "jet_taskgroup_resident_{probe_name}_{}.jet",
                std::process::id()
            ));
            fs::write(&jit_path, probe_source).unwrap();
            let mut bundle = jet::Loader::load_entry(jit_path.to_str().unwrap()).unwrap();
            let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
                .into_iter()
                .filter(|diagnostic| {
                    matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)
                })
                .collect::<Vec<_>>();
            assert!(
                errors.is_empty(),
                "resident probe rejected {probe_name}: {errors:?}"
            );
            let detail = jet_jit::resident_jit_safe_bundle_detail(&bundle);
            assert!(
                jet_jit::resident_jit_safe_bundle(&bundle),
                "{probe_name} must stay resident-JIT safe: {detail}"
            );
            let _ = fs::remove_file(&jit_path);
        })
        .expect("spawn resident JIT probe");
    probe.join().expect("resident JIT probe panicked");

    let (code, stdout, stderr) = run_default_multi(name, "main.jet", &[("main.jet", source)]);
    assert_eq!(code, 0, "resident JIT: {stderr}");
    assert!(
        stderr
            .lines()
            .all(|line| line.contains("tier0 interp") || line.contains("tier1 native")),
        "runtime diagnostics: {stderr}"
    );
    assert_eq!(stdout, expected_stdout, "resident JIT output drifted");

    if have_rustc() {
        let (code, stdout, stderr) = build_and_run_full("jet_taskgroup", name, source);
        assert_eq!(code, 0, "AOT: {stderr}");
        assert_eq!(stderr, "");
        assert_eq!(stdout, expected_stdout, "AOT output drifted");
    }
}

#[test]
fn native_cancellation_closes_group_before_caller_continues() {
    let source = r#"
use core.time as time

fn wait_in_group(sender: Sender<Int>) {
    task.group group {
        child :: task {
            sender.send(1)
            time.sleep(10)
            print("settled")
        }
        time.sleep(10000)
        child.join() ?? panic("child failed")
    }
}

fn run() {
    (sender, ready) :: tasks.channel<Int>()
    outer :: task wait_in_group(sender)
    ready.receive() ?? panic("child did not start")
    outer.cancel()
    result :: outer.join()
    if result == {
        .Err(_) -> { print("cancelled") }
        .Ok(_) -> { print("ok") }
    }
    print("caller")
}
"#;
    assert_group_close_success("taskgroup_cancel", source, "settled\ncancelled\ncaller\n");
}

#[test]
fn native_deadline_closes_group_before_caller_continues() {
    let source = r#"
use core.time as time

fn leave_on_deadline() {
    task.group group {
        child :: task {
            total := 0
            loop n, 0..<2000000 { total += n }
            print("settled")
        }
        #Context(deadline: time.now() - 1) {
            time.sleep(10000)
        }
        child.join() ?? panic("child failed")
    }
}

fn run() {
    leave_on_deadline()
    print("caller")
}
"#;
    assert_native_wait_exit("taskgroup_deadline_exit", source, "E3003", "settled\n");
}

#[test]
fn native_panicked_wait_closes_group_before_caller_continues() {
    let source = r#"
fn slow_value(gate: Shared<[Int]>) => Int {
    gate.edit((state: [Int]) => state[0] = 1)
    total := 0
    loop n, 0..<2000000 { total += n }
    print("settled")
    return 1
}

fn fail_after_start(gate: Shared<[Int]>) => Int {
    loop gate.read((state: [Int]) => state[0]) == 0 {}
    panic("wait failed")
    return 0
}

fn leave_on_wait_panic() {
    gate :: Shared.new([0])
    task.group group {
        slow :: task slow_value(gate)
        ignored :: task.any { fail_after_start(gate) }
        slow.join() ?? panic("slow child failed")
    }
}

fn run() {
    leave_on_wait_panic()
    print("caller")
}
"#;
    assert_group_close_success("taskgroup_wait_panic", source, "settled\ncaller\n");
}

#[test]
fn local_task_binding_is_not_parsed_as_spawn_syntax() {
    let source = r#"
fn run() {
    task :: [7]
    print(task[0])
}
"#;
    let (code, stdout, stderr) = run_default_multi(
        "task_local_binding",
        "main.jet",
        &[("main.jet", source)],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "7\n", "{stderr}");
}

#[test]
fn early_return_closes_group_before_caller_continues() {
    let source = r#"
fn spawn_bad(group: TaskGroup) {
    bad :: task panic("child")
}

fn leave() => Int {
    task.group group {
        spawn_bad(group)
        total := 0
        loop n, 0..<2000000 { total += n }
        return 1
    }
    return 0
}

fn run() {
    leave()
    print("after")
}
"#;
    let (interpreted_stdout, interpreted_stderr) = match interpreter_outcome("early_return", source) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "{stderr}");
            (stdout, stderr)
        }
        RunOutcome::Problems(diags) => panic!("interpreter rejected child panic: {diags:?}"),
    };
    assert_eq!(interpreted_stdout, "after\n");
    assert_eq!(interpreted_stderr, "");

    let (code, stdout, stderr) =
        run_default_multi("taskgroup_early_return", "main.jet", &[("main.jet", source)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "after\n", "{stderr}");
    assert!(
        stderr
            .lines()
            .all(|line| line.contains("tier0 interp") || line.contains("tier1 native")),
        "runtime diagnostics: {stderr}"
    );

    if have_rustc() {
        let (code, stdout, stderr) =
            build_and_run_full("jet_taskgroup", "taskgroup_early_return", source);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "after\n", "{stderr}");
        assert_eq!(stderr, "", "{stderr}");
    }
}
