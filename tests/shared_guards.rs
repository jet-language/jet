//! D-SHAREDGUARD1=A: expert Shared guards keep one meaning across execution tiers.

mod common;

use std::fs;
use std::process::Command;
use jet::JitBackend::JitBackend;

const QUEUE: &str = include_str!("../examples/features/memory/shared_guard_queue.jet");

const CANCEL_WAIT: &str = r#"
use core.tasks as tasks

fn mark_started(started: Shared<Int>, began: Condition) {
    started_guard :: started.guard_edit()
    started_guard.value = 1
    began.notify_all()
}

fn wait_until_cancel(
    shared: Shared<Int>,
    changed: Condition,
    started: Shared<Int>,
    began: Condition
) => Int {
    mark_started(started, began)
    guard :: shared.guard_edit()
    guard.wait(changed, value => value == 1) ?? panic("wait failed")
    return 1
}

fn finish_after_start(started: Shared<Int>, began: Condition) => Int {
    started_guard :: started.guard_edit()
    started_guard.wait(began, value => value == 1) ?? panic("start failed")
    return 7
}

fn run() {
    shared := Shared.new(0)
    changed := Condition.new()
    started := Shared.new(0)
    began := Condition.new()
    task.group workers {
        print(task.race { wait_until_cancel(shared, changed, started, began), finish_after_start(started, began) })
    }
    reacquired :: shared.guard_edit()
    reacquired.value += 1
    print(reacquired.value)
}
"#;

const EARLY_EXITS: &str = r#"
fn return_early(shared: Shared<Int>) {
    guard :: shared.guard_edit()
    guard.value += 1
    return
}

fn fail_early(shared: Shared<Int>) => Int ? String {
    guard :: shared.guard_edit()
    guard.value += 1
    return .Err("stop")
}

fn run() {
    shared := Shared.new(0)
    return_early(shared)
    _ :: fail_early(shared) ?? 0
    guard :: shared.guard_edit()
    guard.value += 1
    print(guard.value)
}
"#;

const HELPERS: &str = r#"
fn inspect(guard: SharedGuard<Int>) => Int {
    return guard.value
}

fn bump(&guard: SharedGuard<Int>) {
    guard.value += 1
}

fn run() {
    shared := Shared.new(4)
    guard := shared.guard_edit()
    print(inspect(guard))
    bump(&guard)
    print(guard.value)
}
"#;

const RETURNED_GUARD: &str = r#"
fn acquire(shared: Shared<Int>) => SharedGuard<Int> {
    return shared.guard_edit()
}

fn read_returned(shared: Shared<Int>) => Int {
    guard :: acquire(shared)
    return guard.value
}

fn run() {
    shared := Shared.new(6)
    print(read_returned(shared))
    shared.edit(value => value += 1)
    print(shared.read(value => value))
}
"#;

const TRANSACTION_DELTAS: &str = r#"
use core.tasks as tasks

struct Counter {
    value: Int
}

fn increment(counter: Shared<Counter>) {
    #Transact(tx) {
        counter.edit(value => value.value += 1)
    }
}

fn run() {
    counter := Shared.new(Counter.{ value: 0 })
    task.group workers {
        first := task increment(counter)
        second := task increment(counter)
        first.join().drop("waits for the transaction to land; the task body already ran to completion")
        second.join().drop("waits for the transaction to land; the task body already ran to completion")
    }
    guard := counter.guard_read()
    print(guard.value.value)
}
"#;

const MAP_AND_SPLIT: &str = r#"
struct Pair {
    left: Int,
    right: Int,
}

fn map_left(shared: Shared<Pair>) {
    mapped := shared.guard_edit().map(value => value.left)
    mapped.value += 10
}

fn split_pair(shared: Shared<Pair>) {
    (left, right) := shared.guard_edit().split(
        value => value.left,
        value => value.right
    )
    left.value += 1
    right.value += 2
}

fn run() {
    mapped := Shared.new(Pair.{ left: 1, right: 2 })
    map_left(mapped)
    print(mapped.read(value => value.left + value.right))

    split := Shared.new(Pair.{ left: 3, right: 4 })
    split_pair(split)
    print(split.read(value => value.left + value.right))
}
"#;

const STORED_GUARDS: &str = r#"
struct HeldGuard {
    stored: (lease: SharedGuard<Int>, marker: Int),
}

enum Held {
    Guard(SharedGuard<Int>)
}

fn ignore_union(value: SharedGuard<Int> | Int) => Int {
    return 0
}

fn read_stored(shared: Shared<Int>) => Int {
    HeldGuard.{ stored } :: HeldGuard.{
        stored: (lease: shared.guard_edit(), marker: 0),
    }
    (lease, marker) :: stored
    return lease.value + marker
}

fn acquire_pair(
    first: Shared<Int>,
    second: Shared<Int>
) => (left: SharedGuard<Int>, right: SharedGuard<Int>) {
    return (left: first.guard_edit(), right: second.guard_edit())
}

fn read_pair(first: Shared<Int>, second: Shared<Int>) => Int {
    (left, right) :: acquire_pair(first, second)
    return left.value + right.value
}

fn run() {
    first := Shared.new(8)
    second := Shared.new(2)
    print(read_stored(first))
    print(read_pair(first, second))
    first.edit(value => value += 1)
    print(first.read(value => value))
}
"#;

fn fixture(tag: &str, source: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = common::unique_tmp(tag);
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, source).unwrap();
    (root, path)
}

fn assert_native_and_default(source: &str, expected: &str, tag: &str) {
    assert!(common::have_rustc(), "SharedGuard parity proof needs rustc");

    let (native_root, native_path) = fixture(&format!("{tag}_native"), source);
    let compiled = jet::compile_with_path(source, native_path.to_str().unwrap())
        .unwrap_or_else(|diagnostics| {
            panic!(
                "{}",
                jet::render_diagnostics(native_path.to_str().unwrap(), source, &diagnostics)
            )
        });
    let rust_path = native_root.join("main.rs");
    let native_bin = native_root.join("main");
    fs::write(&rust_path, compiled.rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rust_path)
        .arg("-o")
        .arg(&native_bin)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "rustc rejected SharedGuard output:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let native = Command::new(native_bin).output().unwrap();
    assert!(native.status.success(), "{native:?}");
    assert_eq!(String::from_utf8(native.stdout).unwrap(), expected);

    let (_, default_path) = fixture(&format!("{tag}_default"), source);
    let mut bundle = jet::Loader::load_entry(default_path.to_str().unwrap()).unwrap();
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

    let mut backend = jet_jit::CraneliftBackend::new();
    jet_jit::reset_jit_trace_for_test();
    match backend.run(&bundle, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, .. } => {
            assert!(
                jet_jit::deopt_invoked_for_test(),
                "SharedGuard must deopt until the JIT can marshal the full Prelude protocol"
            );
            assert_eq!(stdout, expected);
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("default SharedGuard tier failed: {diagnostics:?}")
        }
    }
}

fn with_compiler_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("shared-guard-parity".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn shared_guard_queue_matches_native_and_default_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(QUEUE, "7\n", "jet_shared_guard_queue")
    });
}

#[test]
fn cancelling_condition_wait_unregisters_and_releases_guard() {
    with_compiler_stack(|| {
        assert_native_and_default(CANCEL_WAIT, "7\n1\n", "jet_shared_guard_cancel")
    });
}

#[test]
fn return_and_error_paths_release_guard() {
    with_compiler_stack(|| {
        assert_native_and_default(EARLY_EXITS, "3\n", "jet_shared_guard_early_exit")
    });
}

#[test]
fn named_guard_spans_read_and_write_helpers_on_all_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(HELPERS, "4\n5\n", "jet_shared_guard_helpers")
    });
}

#[test]
fn returned_named_guard_reads_and_releases_on_all_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(
            RETURNED_GUARD,
            "6\n7\n",
            "jet_shared_guard_returned",
        )
    });
}

#[test]
fn concurrent_transaction_deltas_apply_to_fresh_locked_state_on_all_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(
            TRANSACTION_DELTAS,
            "2\n",
            "jet_shared_guard_transaction_deltas",
        )
    });
}

#[test]
fn mapped_and_split_guards_write_disjoint_fields_on_all_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(MAP_AND_SPLIT, "13\n10\n", "jet_shared_guard_projection")
    });
}

#[test]
fn stored_and_returned_guards_move_as_read_capabilities_on_all_tiers() {
    with_compiler_stack(|| {
        assert_native_and_default(
            STORED_GUARDS,
            "8\n10\n9\n",
            "jet_shared_guard_storage",
        )
    });
}
