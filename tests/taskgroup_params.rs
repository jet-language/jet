#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, build_and_run_full, have_rustc, run_default_multi};
use jet::Interpreter::{dev_iteration, RunOutcome};
use std::fs;

const OUTER_GROUP_HELPER: &str = r#"
fn spawn_later(group: TaskGroup) => Shared<[Int]> {
    gate :: Shared.new([0])
    group.task => {
        gate.edit((state: [Int]) => state[0] = state[0] + 1)
        loop gate.read((state: [Int]) => state[0]) == 1 {}
        gate.edit((state: [Int]) => state[0] = state[0] + 1)
        total := 0
        loop n; 0..<2000000 { total += n }
        print("task")
    }
    loop gate.read((state: [Int]) => state[0]) == 0 {}
    return ~gate
}

fn run() {
    taskgroup group {
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
    let path = std::env::temp_dir().join(format!(
        "jet_taskgroup_jit_{name}_{}.jet",
        std::process::id()
    ));
    fs::write(&path, source).unwrap();
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
    jet_jit::try_compile_bundle(&bundle).expect("TaskGroup source must compile for resident JIT");
}

#[test]
fn named_function_parameters_spawn_copy_and_owned_captures() {
    if !have_rustc() {
        return;
    }
    let source = r#"
fn spawn_one(group: TaskGroup, value: Int) {
    task :: group.task => value + 1
    print(task.join())
}

fn spawn_owned(group: TaskGroup, values: ^[Int]) {
    task :: group.task => values[0]
    print(task.join())
}

fn spawn_both(first: TaskGroup, second: TaskGroup) {
    left :: first.task => 20
    right :: second.task => 22
    print(left.join() + right.join())
}

fn run() {
    taskgroup group {
        spawn_one(group, 41)
        values :: [7, 8, 9]
        spawn_owned(group, ^values)
        taskgroup inner {
            spawn_both(group, inner)
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
    task :: group.task => values[0]
    print(task.join())
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
    taskgroup all_group {
        one :: all_group.task => 1
        two :: all_group.task => 2
        values :: all_group.all([one, two])
        print(values[0] + values[1])
    }
    taskgroup race_group {
        seven :: race_group.task => slow_seven()
        eight :: race_group.task => 8
        print(race_group.race([seven, eight]))
    }
    taskgroup any_group {
        eleven :: any_group.task => slow_eleven()
        twelve :: any_group.task => 12
        print(any_group.any([eleven, twelve]))
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
        RunOutcome::Problems(diags) => panic!("interpreter rejected taskgroups: {diags:?}"),
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
) {
    assert_jit_compiles(name, source);
    let (code, stdout, stderr) = run_default_multi(name, "main.jet", &[("main.jet", source)]);
    assert_ne!(code, 0, "{stderr}");
    assert_eq!(stdout, "", "{stderr}");
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

#[test]
fn native_cancellation_closes_group_before_caller_continues() {
    let source = r#"
use core.tasks as tasks
use core.time as time

fn wait_in_group(gate: Shared<[Int]>) {
    taskgroup group {
        child :: group.task => {
            gate.edit((state: [Int]) => state[0] = 1)
            total := 0
            loop n; 0..<2000000 { total += n }
            print("settled")
        }
        time.sleep(10000)
        child.join()
    }
}

fn run() {
    gate :: Shared.new([0])
    outer :: tasks.spawn(() => wait_in_group(gate))
    loop gate.read((state: [Int]) => state[0]) == 0 {}
    outer.cancel()
    outer.join()
    print("caller")
}
"#;
    assert_native_wait_exit("taskgroup_cancel_exit", source, "cancel");
}

#[test]
fn native_deadline_closes_group_before_caller_continues() {
    let source = r#"
use core.time as time

fn leave_on_deadline() {
    taskgroup group {
        child :: group.task => {
            total := 0
            loop n; 0..<2000000 { total += n }
            print("settled")
        }
        #Context(deadline: time.now() - 1) {
            time.sleep(10000)
        }
        child.join()
    }
}

fn run() {
    leave_on_deadline()
    print("caller")
}
"#;
    assert_native_wait_exit("taskgroup_deadline_exit", source, "E3003");
}

#[test]
fn native_panicked_wait_closes_group_before_caller_continues() {
    let source = r#"
fn slow_value(gate: Shared<[Int]>) => Int {
    gate.edit((state: [Int]) => state[0] = 1)
    total := 0
    loop n; 0..<2000000 { total += n }
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
    taskgroup group {
        slow :: group.task => slow_value(gate)
        failed :: group.task => fail_after_start(gate)
        ignored :: group.any([failed])
        slow.join()
    }
}

fn run() {
    leave_on_wait_panic()
    print("caller")
}
"#;
    assert_native_wait_exit("taskgroup_wait_panic_exit", source, "wait failed");
}

#[test]
fn early_return_closes_group_before_caller_continues() {
    let source = r#"
fn spawn_bad(group: TaskGroup) {
    bad :: group.task => panic("child")
}

fn leave() => Int {
    taskgroup group {
        spawn_bad(group)
        total := 0
        loop n; 0..<2000000 { total += n }
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
            assert_eq!(exit_code, 70, "{stderr}");
            (stdout, stderr)
        }
        RunOutcome::Problems(diags) => panic!("interpreter rejected child panic: {diags:?}"),
    };
    assert!(!interpreted_stdout.contains("after"), "{interpreted_stdout:?}");
    assert!(interpreted_stderr.starts_with("panic: child\n"), "{interpreted_stderr}");

    let (code, stdout, stderr) =
        run_default_multi("taskgroup_early_return", "main.jet", &[("main.jet", source)]);
    assert_eq!(code, 70, "{stderr}");
    assert!(!stdout.contains("after"), "{stdout:?}\n{stderr}");
    assert!(stderr.contains("panic: child\n"), "{stderr}");

    if have_rustc() {
        let (code, stdout, stderr) =
            build_and_run_full("jet_taskgroup", "taskgroup_early_return", source);
        assert_eq!(code, 70, "{stderr}");
        assert!(!stdout.contains("after"), "{stdout:?}\n{stderr}");
        assert!(stderr.starts_with("panic: child\n"), "{stderr}");
    }
}
