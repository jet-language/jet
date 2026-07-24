fn error_codes(source: &str) -> Vec<String> {
    jet::compile(source)
        .expect_err("adversarial data-race source must not compile")
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn assert_rejected(source: &str, code: &str) {
    let codes = error_codes(source);
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

#[test]
fn mutable_state_cannot_cross_task_or_taskgroup_boundaries() {
    assert_rejected(
        r#"
use core.tasks
fn run() {
    count := 0
    task :: tasks.spawn(() => {
        count += 1
    })
    task.join()
}
"#,
        "E1101",
    );

    assert_rejected(
        r#"
fn run() {
    count := 0
    taskgroup group {
        task :: group.task(() => {
            count += 1
        })
        task.join()
    }
}
"#,
        "E1101",
    );
}

#[test]
fn mutable_view_cannot_cross_a_channel_boundary() {
    assert_rejected(
        r#"
use core.tasks
fn run() {
    values := [1, 2, 3]
    edit :: &values[0..1]
    (sender, receiver) :: tasks.channel<ViewMut<Int>>()
    sender.send(edit)
}
"#,
        "E1102",
    );
}

#[test]
fn every_parallel_adapter_rejects_mutable_captures() {
    let cases = [
        (
            "para_map",
            r#"
fn run() {
    seen: [Int] := []
    values :: [1, 2, 3]
    values.para_map((n: Int) => {
        seen.push(n)
    })
}
"#,
        ),
        (
            "para_filter",
            r#"
fn run() {
    seen: [Int] := []
    values :: [1, 2, 3]
    values.para_filter((n: Int) => {
        seen.push(n)
    })
}
"#,
        ),
        (
            "para_partition",
            r#"
fn run() {
    seen: [Int] := []
    values :: [1, 2, 3]
    values.para_partition((n: Int) => {
        seen.push(n)
    })
}
"#,
        ),
        (
            "para_fold",
            r#"
fn run() {
    seen: [Int] := []
    values :: [1, 2, 3]
    values.para_fold(
        () => 0,
        (total: Int, n: Int) => {
            seen.push(n)
        },
        (left: Int, right: Int) => left + right
    )
}
"#,
        ),
    ];

    for (method, source) in cases {
        let codes = error_codes(source);
        assert!(
            codes.iter().any(|code| code == "E1111"),
            "{method} must reject mutable captures with E1111, got {codes:?}"
        );
    }
}

#[test]
fn shared_is_the_safe_mutation_control_case() {
    let source = r#"
use core.tasks
struct Counter { value: Int }
fn run() {
    counter := Shared.new(Counter.{ value: 0 })
    task :: tasks.spawn(() => {
        counter.edit((value) => {
            value.value += 1
        })
    })
    task.join()
    print(counter.read((value) => value.value))
}
"#;

    let output = jet::compile(source).expect("Shared<T> must cross a task through its lock");
    assert!(
        output.rust.contains("JetShared"),
        "safe control must lower through the synchronized Shared<T> runtime"
    );
}
