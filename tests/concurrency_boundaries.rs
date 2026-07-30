fn error_codes(source: &str) -> Vec<String> {
    jet::compile(source)
        .expect_err("adversarial data-race source must not compile")
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn architecture_states_datarace1_c_guarantee() {
    let architecture = include_str!("../docs/spec/architecture.md");
    assert!(
        architecture.contains("D-DATARACE1=C is law"),
        "architecture must state the ratified D-DATARACE1=C guarantee"
    );
    assert!(
        architecture.contains("lock-ordered `Arc` storage")
            || architecture.contains("lock-ordered Arc storage"),
        "architecture must state synchronized reactive storage"
    );
    assert!(
        !architecture.contains("own the open choice"),
        "architecture must not treat D-DATARACE1 as an open choice after ratification"
    );
    assert!(
        !architecture.contains("leans on rustc"),
        "architecture must not leave reactive crossings on a rustc Send backstop"
    );
    assert_eq!(
        jet_foundation::Syntax::ATTR_LOCAL,
        "Local",
        "#Local must be registered in Syntax.rs (D-DATARACE1=C / I7)"
    );
    assert_eq!(
        jet_foundation::Syntax::ATTR_SHARED,
        "Shared",
        "#Shared must be registered in Syntax.rs (D-DATARACE1=C / I7)"
    );
}

fn assert_rejected(source: &str, code: &str) {
    let codes = error_codes(source);
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

#[test]
fn local_cell_rejects_task_channel_shared_and_parallel_crossings() {
    assert_rejected(
        r#"
use core.tasks as tasks
fn cross(cell: Cell<Int>) {
    worker :: tasks.spawn(() => {
        _ :: cell
    })
    worker.join()
}
fn run() {}
"#,
        "E1102",
    );

    assert_rejected(
        r#"
use core.tasks as tasks
fn cross(cell: Cell<Int>) {
    (tx, rx) :: tasks.channel<Cell<Int>>()
    tx.send(^cell)
}
fn run() {}
"#,
        "E1102",
    );

    assert_rejected(
        r#"
fn cross(cell: Cell<Int>) {
    values :: [1, 2, 3]
    _ :: values.para_map((n: Int) => {
        _ :: cell
        return n
    })
}
fn run() {}
"#,
        "E1111",
    );
}

#[test]
fn shared_constructor_rejects_local_cell_at_the_constructor() {
    let source = r#"
fn cross(cell: Cell<Int>) {
    _ :: Shared.new(^cell)
}
fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("Shared.new(Cell) must fail in sema");
    let error = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "E1102"
                && diagnostic.what.contains("cannot be stored in `Shared<T>`")
        })
        .expect("E1102 must point at Shared.new instead of a later task use");
    let start = error.span.expect("E1102 must have a source span").start;
    assert!(
        source[start..].starts_with("^cell") || source[start..].starts_with("cell"),
        "E1102 must point at Shared.new's Cell argument: {error:?}"
    );
}

#[test]
fn shared_constructor_rejects_cell_nested_in_a_struct() {
    let source = r#"
struct Cache { value: Cell<Int> }
fn cross(cache: Cache) {
    _ :: Shared.new(^cache)
}
fn run() {}
"#;
    let diagnostics =
        jet::compile(source).expect_err("Shared.new(struct containing Cell) must fail in sema");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1102"
                && diagnostic.what.contains("cannot be stored in `Shared<T>`")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn task_capture_rejects_cell_nested_in_a_struct() {
    let source = r#"
use core.tasks as tasks
struct Cache { value: Cell<Int> }
fn cross(cache: Cache) {
    worker :: tasks.spawn(() => {
        _ :: cache
    })
    worker.join()
}
fn run() {}
"#;
    assert_rejected(source, "E1102");
}

#[test]
fn local_cell_guard_types_are_not_sendable() {
    for guard in ["CellReadGuard<Int>", "CellEditGuard<Int>"] {
        let source = format!(
            r#"
use core.tasks as tasks
fn cross(guard: {guard}) {{
    worker :: tasks.spawn(() => {{
        _ :: guard
    }})
    worker.join()
}}
fn run() {{}}
"#
        );
        let codes = error_codes(&source);
        assert!(
            codes.iter().any(|code| code == "E1102"),
            "task capture must keep the task-boundary diagnostic: {codes:?}"
        );
        assert!(
            codes.iter().all(|code| code != "E0215"),
            "aggregate-storage E0215 must not shadow task-boundary E1102: {codes:?}"
        );
    }
}

#[test]
fn signal_crosses_task_and_channel_without_rustc_send_ice() {
    let task_source = r#"
use core.reactive as reactive
use core.tasks as tasks
fn run() {
    pending := reactive.signal(0)
    worker :: tasks.spawn(() => {
        pending.set(1)
    })
    worker.join()
    print(pending.get())
}
"#;
    let out = jet::compile(task_source).expect("Signal must cross tasks via lock-ordered Arc");
    assert!(
        out.rust.contains("JetSignal"),
        "Signal task crossing must lower through JetSignal"
    );
    assert!(
        out.rust.contains("std::sync::Arc") || out.rust.contains("RwLock"),
        "Signal lowering must use synchronized storage"
    );
    assert!(
        out.rust.contains("jet-reactive-upgrade:"),
        "Signal task crossing must emit an upgrade report comment"
    );

    let channel_source = r#"
use core.reactive as reactive
use core.tasks as tasks
fn run() {
    pending := reactive.signal(0)
    (tx, rx) :: tasks.channel<Signal<Int>>()
    tx.send(~pending)
    got :: rx.receive() ?? panic("recv")
    print(got.get())
}
"#;
    let out = jet::compile(channel_source).expect("Signal must cross channels via lock-ordered Arc");
    assert!(
        out.rust.contains("JetSignal"),
        "Signal channel crossing must lower through JetSignal"
    );
    assert!(
        out.rust.contains("jet-reactive-upgrade:"),
        "Signal channel crossing must emit an upgrade report comment"
    );
}

#[test]
fn parallel_adapter_allows_synchronized_reactive_capture() {
    let source = r#"
use core.reactive as reactive
fn run() {
    pending := reactive.signal(0)
    values :: [1, 2, 3]
    _ :: values.para_map((n: Int) => pending.get() + n)
}
"#;
    let out = jet::compile(source).expect("Signal may cross para_* via lock-ordered Arc");
    assert!(
        out.rust.contains("JetSignal"),
        "parallel reactive capture must lower through JetSignal"
    );
    assert!(
        out.rust.contains("jet-reactive-upgrade:"),
        "parallel reactive crossing must emit an upgrade report comment"
    );
}

#[test]
fn derived_and_computed_cross_task_without_rustc_send_ice() {
    let derived_source = r#"
use core.reactive as reactive
use core.tasks as tasks
fn run() {
    base := reactive.signal(1)
    twice := reactive.derived(() => (base.get() * 2))
    worker :: tasks.spawn(() => {
        print(twice.get())
    })
    worker.join()
}
"#;
    let out = jet::compile(derived_source).expect("Derived must cross tasks via lock-ordered Arc");
    assert!(
        out.rust.contains("JetDerived"),
        "Derived task crossing must lower through JetDerived"
    );
    assert!(
        out.rust.contains("jet-reactive-upgrade:"),
        "Derived task crossing must emit an upgrade report comment"
    );

    let computed_source = r#"
use core.reactive as reactive
use core.tasks as tasks
fn run() {
    base := reactive.signal(1)
    twice := reactive.computed(() => (base.get() * 2))
    (tx, rx) :: tasks.channel<Computed<Int>>()
    tx.send(~twice)
    got :: rx.receive() ?? panic("recv")
    print(got.get())
}
"#;
    let out =
        jet::compile(computed_source).expect("Computed must cross channels via lock-ordered Arc");
    assert!(
        out.rust.contains("JetDerived") || out.rust.contains("Computed"),
        "Computed channel crossing must lower through the derived/computed runtime"
    );
    assert!(
        out.rust.contains("jet-reactive-upgrade:"),
        "Computed channel crossing must emit an upgrade report comment"
    );
}

#[test]
fn local_pin_rejects_reactive_task_crossing() {
    assert_rejected(
        r#"
use core.reactive as reactive
use core.tasks as tasks
fn run() {
    #Local pending := reactive.signal(0)
    worker :: tasks.spawn(() => {
        pending.set(1)
    })
    worker.join()
}
"#,
        "E1102",
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
        task :: group.task => {
            count += 1
        }
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
    seen := [Int].{}
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
    seen := [Int].{}
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
    seen := [Int].{}
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
    seen := [Int].{}
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
