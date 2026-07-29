#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

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
    ] {
        assert_eq!(error_codes(source), expected);
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
