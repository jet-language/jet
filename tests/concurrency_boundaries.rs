mod common;

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
        jet_foundation::Syntax::MARKER_LOCAL,
        "Local",
        "#Local must be registered in Syntax.rs (D-DATARACE1=C / I7)"
    );
    assert_eq!(
        jet_foundation::Syntax::MARKER_SHARED,
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
fn shared_guard_cannot_cross_a_task_boundary() {
    assert_rejected(
        r#"
fn run() {
    shared := Shared.new(1)
    guard :: shared.guard_read()
    worker :: task print(guard.value)
    worker.join() ?? panic("task failed")
}
"#,
        "E1102",
    );
}

#[test]
fn local_cell_rejects_task_channel_shared_and_parallel_crossings() {
    assert_rejected(
        r#"
fn cross(cell: Cell<Int>) {
    worker :: task {
        _ :: cell
    }
    worker.join() ?? panic("task failed")
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
struct Cache { value: Cell<Int> }
fn cross(cache: Cache) {
    worker :: task {
        _ :: cache
    }
    worker.join() ?? panic("task failed")
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
fn cross(guard: {guard}) {{
    worker :: task {{
        _ :: guard
    }}
    worker.join() ?? panic("task failed")
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
            codes.iter().all(|code| code != "E0217"),
            "aggregate-storage E0217 must not shadow task-boundary E1102: {codes:?}"
        );
    }
}

#[test]
fn signal_crosses_task_and_channel_without_rustc_send_ice() {
    let task_source = r#"
use core.reactive as reactive
fn run() {
    pending := reactive.signal(0)
    worker :: task {
        pending.set(1)
    }
    worker.join() ?? panic("task failed")
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
fn run() {
    base := reactive.signal(1)
    twice := reactive.derived(() => (base.get() * 2))
    worker :: task {
        print(twice.get())
    }
    worker.join() ?? panic("task failed")
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
fn run() {
    #Local pending := reactive.signal(0)
    worker :: task {
        pending.set(1)
    }
    worker.join() ?? panic("task failed")
}
"#,
        "E1102",
    );
}

#[test]
fn mutable_state_cannot_cross_task_or_taskgroup_boundaries() {
    assert_rejected(
        r#"
fn run() {
    count := 0
    handle :: task {
        count += 1
    }
    handle.join() ?? panic("task failed")
}
"#,
        "E1101",
    );

    assert_rejected(
        r#"
fn run() {
    count := 0
    task.group group {
        handle :: task {
            count += 1
        }
        handle.join() ?? panic("task failed")
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
struct Counter { value: Int }
fn run() {
    counter := Shared.new(Counter.{ value: 0 })
    handle :: task {
        counter.edit((value) => {
            value.value += 1
        })
    }
    handle.join() ?? panic("task failed")
    print(counter.read((value) => value.value))
}
"#;

    let output = jet::compile(source).expect("Shared<T> must cross a task through its lock");
    assert!(
        output.rust.contains("JetShared"),
        "safe control must lower through the synchronized Shared<T> runtime"
    );
}

// D-TASKBORROW1=A (card #1199): a `task.group` joins every child, so a child may
// borrow places the owner still holds. Reads are free; writes are admitted only
// when sema proves the places disjoint. Detached `task` children, channels, and detach keep
// the ownership-only rules.

fn assert_accepted(source: &str) -> jet::CompileOutput {
    match jet::compile(source) {
        Ok(output) => output,
        Err(diagnostics) => panic!(
            "source must compile, got {:?}",
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>()
        ),
    }
}

const PARTICLES: &str = r#"
struct Particle { position: Int, velocity: Int }
"#;

#[test]
fn taskgroup_child_reads_borrowed_stack_data() {
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        window :: particles[0..1]
        a :: task window[0].position
    }}
}}
"#
    );
    assert_accepted(&source);
}

#[test]
fn taskgroup_children_borrow_disjoint_write_places() {
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}}, .{{position: 30, velocity: 4}} }}
        task.group g {{
        left :: &particles[0]
        right :: &particles[2]
        a :: task {{
            left.position += left.velocity
            left.position
        }}
        b :: task {{
            right.position += right.velocity
            right.position
        }}
    }}
}}
"#
    );
    let output = assert_accepted(&source);
    assert!(
        output.rust.contains(".spawn("),
        "a borrowed task group child must use the canonical group spawn path whose loan the group closes at join"
    );
}

#[test]
fn taskgroup_children_writing_one_place_are_rejected() {
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        one :: &particles[0]
        two :: &particles[0]
        a :: task {{
            one.position += 1
            one.position
        }}
        b :: task {{
            two.position += 1
            two.position
        }}
    }}
}}
"#
    );
    assert_rejected(&source, "E1101");
}

#[test]
fn taskgroup_read_and_write_of_one_place_are_rejected() {
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        writer :: &particles[0]
        reader :: particles[0..1]
        a :: task {{
            writer.position += 1
            writer.position
        }}
        b :: task reader[0].position
    }}
}}
"#
    );
    assert_rejected(&source, "E1101");
}

#[test]
fn detached_task_still_rejects_a_borrowed_capture() {
    let source = format!(
        r#"
{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
    left :: &particles[0]
    worker :: task left.position
    print(worker.join() ?? panic("task failed"))
}}
"#
    );
    assert_rejected(&source, "E1102");
}

#[test]
fn nested_taskgroups_track_their_own_borrows() {
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}}, .{{position: 30, velocity: 4}} }}
        task.group outer {{
        left :: &particles[0]
        a :: task {{
            left.position += 1
            left.position
        }}
        task.group inner {{
            right :: &particles[2]
            b :: task {{
                right.position += 1
                right.position
            }}
        }}
    }}
}}
"#
    );
    assert_accepted(&source);
}

#[test]
fn a_group_cannot_lend_an_owner_declared_inside_its_own_block() {
    // The owner drops before the group joins, so the loan could outlive it.
    let source = format!(
        r#"{PARTICLES}
fn run() {{
        task.group g {{
        particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        left :: &particles[0]
        a :: task {{
            left.position += 1
            left.position
        }}
    }}
}}
"#
    );
    assert_rejected(&source, "E1102");
}

#[test]
fn a_lent_owner_cannot_be_moved_before_the_group_joins() {
    // The child still holds the loan, so moving the owner out from under it
    // would free heap the running task reads. `left`'s last lexical use is the
    // spawn, so the ordinary view rules end its borrow too early.
    let source = format!(
        r#"{PARTICLES}
fn eat(ps: ^[Particle]) => Int :: ps.len()
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        left :: &particles[0]
        a :: task {{
            left.position += left.velocity
            left.position
        }}
        n :: eat(^particles)
        print(n)
    }}
}}
"#
    );
    assert_rejected(&source, "E1101");
}

#[test]
fn a_lent_place_cannot_be_written_by_the_parent_before_the_group_joins() {
    // A parent write races the child's write through the same place.
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        left :: &particles[0]
        a :: task {{
            left.position += left.velocity
            left.position
        }}
        particles[0].position = 99
    }}
}}
"#
    );
    assert_rejected(&source, "E1101");
}

#[test]
fn a_write_lent_place_cannot_be_read_by_the_parent_before_the_group_joins() {
    // A parent read races the child's write. Sequentially a read beside a live
    // write view is fine, so this rule is specific to the concurrent case.
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        left :: &particles[0]
        a :: task {{
            left.position += left.velocity
            left.position
        }}
        print(particles[0].position)
    }}
}}
"#
    );
    assert_rejected(&source, "E1101");
}

#[test]
fn a_read_lent_place_stays_readable_by_the_parent() {
    // Shared reads do not race each other, so a read loan constrains nothing.
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
        task.group g {{
        window :: particles[0..1]
        a :: task window[0].position
        print(particles[0].position)
    }}
}}
"#
    );
    assert_accepted(&source);
}

#[test]
fn a_place_the_group_never_lent_stays_writable() {
    // The loan covers `particles[0]`. `particles[1]` is proven disjoint, so the
    // parent may still write it while the child runs.
    let source = format!(
        r#"{PARTICLES}
fn run() {{
    particles := [Particle].{{ .{{position: 10, velocity: 2}}, .{{position: 20, velocity: 3}} }}
    task.group g {{
        left :: &particles[0]
        a :: task {{
            left.position += left.velocity
            left.position
        }}
        particles[1].position = 99
    }}
}}
"#
    );
    assert_accepted(&source);
}

#[test]
fn a_taskgroup_parameter_cannot_lend_a_borrow() {
    // The join runs in the caller's frame, so this frame cannot prove the
    // borrowed owner outlives it.
    let source = r#"
fn spawn_view(group: TaskGroup, values: View<Int>) {
    handle :: task values[0]
    print(handle.join() ?? panic("task failed"))
}
fn run() {}
"#;
    assert_rejected(source, "E1102");
}
