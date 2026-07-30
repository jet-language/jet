#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, compile, have_rustc};

const SOURCE: &str = r#"
struct Pair {
    left: Int,
    right: Int,
}

struct Cache {
    value: Cell<String?>,
}

struct LineCache {
    value: Cell<[Int]?>,
}

fn mapped_read(cell: Cell<Pair>) {
    left :: cell.guard_read().map(pair => pair.left)
    print(left.get())
}

fn split_read(cell: Cell<Pair>) {
    (left, right) :: cell.guard_read().split(
        pair => pair.left,
        pair => pair.right
    )
    print(left.get() + right.get())
}

fn mapped_edit(cell: Cell<Pair>) {
    left :: cell.guard_edit().map(pair => pair.left)
    left.set(9)
}

fn split_edit(cell: Cell<Pair>) {
    (left, right) :: cell.guard_edit().split(
        pair => pair.left,
        pair => pair.right
    )
    left.set(10)
    right.set(11)
}

fn make_edit_guards(cell: Cell<Pair>) => (
    first: CellEditGuard<Int>,
    second: CellEditGuard<Int>
) {
    return cell.guard_edit().split(
        pair => pair.left,
        pair => pair.right
    )
}

fn edit_returned_split(cell: Cell<Pair>) {
    (left, right) :: make_edit_guards(cell)
    left.set(12)
    right.set(13)
}

fn edit_then_return(cell: Cell<Int>) {
    guard :: cell.guard_edit()
    guard.set(4)
    return
}

fn run() {
    cell :: Cell.new(Pair.{ left: 1, right: 2 })
    print(cell.read(pair => pair.left + pair.right))
    cell.edit(pair => pair.left += 3)
    print(cell.get().left)
    old :: cell.replace(Pair.{ left: 5, right: 6 })
    print(old.left)
    cell.set(Pair.{ left: 7, right: 8 })
    mapped_read(cell)
    split_read(cell)
    mapped_edit(cell)
    print(cell.get().left)
    split_edit(cell)
    print(cell.get().left + cell.get().right)
    edit_returned_split(cell)
    print(cell.get().left + cell.get().right)

    cache :: Cache.{ value: Cell.new(None) }
    print(cache.value.get_or_set(() => "built"))
    print(cache.value.get_or_set(() => "unused"))

    lines :: LineCache.{ value: Cell.new(None) }
    print(lines.value.get_or_set(() => [0, 8, 15]).len())
    print(lines.value.get_or_set(() => [99]).len())

    early :: Cell.new(1)
    edit_then_return(early)
    print(early.get())
    early.edit(value => value += 1)
    print(early.get())

    wide :: Cell.new(U16.{0})
    wide.set(U8.{7})
    print(wide.get() == U16.{7})

    decimal :: Cell.new(Float.{0.0})
    decimal.set(Int.{42})
    print(decimal.get() == 42.0)
}
"#;

const EXPECTED: &str =
    "3\n4\n4\n7\n15\n9\n21\n25\nbuilt\nbuilt\n3\n3\n4\n5\ntrue\ntrue\n";

const GENERIC_SOURCE: &str = r#"
struct Box<T> {
    value: T,
}

struct Number {
    value: Int,
}

struct Reverse<Value, Alpha> {
    first: Value,
    second: Alpha,
}

impl Box {
    fn new(value: ^T) => Box<T> {
        return Box<T>.{ value: value }
    }
}

fn keep_result(value: ^(Int ? String)) {
    cell :: Cell.new(value)
    print(cell.read(result => result ?? 0))
}

fn ok_result() => Int ? String {
    return Ok(7)
}

fn run() {
    counts := [String: Int].{}
    counts["jet"] = 3
    map_cell :: Cell.new(^counts)
    print(map_cell.read(values => values.get("jet") ?? 0))

    keep_result(ok_result())

    shared :: Shared.new(9)
    shared_cell :: Cell.new(shared)
    print(shared_cell.read(handle => handle.read(value => value)))

    nested :: Cell.new(Box<Box<Int>>.new(Box<Int>.new(11)))
    projected :: nested.guard_read().map(value => value.value.value)
    print(projected.get())

    reverse :: Cell.new(Reverse<Float, Number>.{
        first: 1.5,
        second: Number.{ value: 13 },
    })
    print(reverse.get().first)
    print(reverse.get().second.value)

}
"#;

const GENERIC_EXPECTED: &str = "3\n7\n9\n11\n1.5\n13\n";

const INT_MAP_SOURCE: &str = r#"
fn run() {
    values := [Int: Int].{}
    values[1] = 2
    cell :: Cell.new(^values)
    print(cell.read(items => items.get(1) ?? 0))
}
"#;

#[test]
fn local_cell_split_keeps_projected_tuple_type_in_tir() {
    let rust = compile("local_cell_split_type", SOURCE);
    assert!(
        rust.contains("let user_left = (__jet_d")
            && rust.contains(".user_first;")
            && rust.contains(".user_second;"),
        "split TIR lost its exact projected tuple fields:\n{rust}"
    );
    assert!(
        !rust.contains("#[derive(Clone, PartialEq)]\nstruct JetTup_d9655a63806bd711"),
        "the read-guard tuple must stay move-only"
    );
}

#[test]
fn local_cell_full_surface_runs_through_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("local_cell_aot", SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn local_cell_full_surface_runs_through_default_tier() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let dir = std::env::temp_dir().join(format!(
                "jet_local_cell_parity_{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("main.jet");
            fs::write(&path, SOURCE).unwrap();
            let shown = path.to_string_lossy().into_owned();

            for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
                jet_jit::reset_jit_trace_for_test();
                match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
                    jet::Interpreter::RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    } => {
                        assert_eq!(exit_code, 0, "{tier} exit drift");
                        assert_eq!(stderr, "", "{tier} stderr drift");
                        assert_eq!(stdout, EXPECTED, "{tier} output drift");
                    }
                    jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                        panic!("{tier} rejected Cell: {diagnostics:?}")
                    }
                }
                if force_interpreter {
                    continue;
                }
                assert!(
                    jet_jit::jit_executed_for_test(),
                    "Cell must execute native resident JIT code"
                );
                assert!(
                    !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
                    "Cell must not deopt or use fallback"
                );
            }
        })
        .expect("spawn 2 MiB local Cell embedder")
        .join()
        .expect("local Cell embedder must not overflow");
}

#[test]
fn local_cell_generic_shapes_run_through_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("local_cell_generic_aot", GENERIC_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, GENERIC_EXPECTED);
}

#[test]
fn local_cell_generic_shapes_run_through_default_tier() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let dir = std::env::temp_dir().join(format!(
                "jet_local_cell_generic_parity_{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("main.jet");
            fs::write(&path, GENERIC_SOURCE).unwrap();
            let shown = path.to_string_lossy().into_owned();

            for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
                jet_jit::reset_jit_trace_for_test();
                match jet::Interpreter::dev_iteration(&shown, false, force_interpreter) {
                    jet::Interpreter::RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    } => {
                        assert_eq!(exit_code, 0, "{tier} exit drift");
                        assert_eq!(stderr, "", "{tier} stderr drift");
                        assert_eq!(stdout, GENERIC_EXPECTED, "{tier} output drift");
                    }
                    jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                        panic!("{tier} rejected generic Cell: {diagnostics:?}")
                    }
                }
                if force_interpreter {
                    continue;
                }
                assert!(
                    jet_jit::jit_executed_for_test(),
                    "generic Cell shapes must execute native resident JIT code"
                );
                assert!(
                    !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
                    "generic Cell shapes must not deopt or use fallback"
                );
            }
        })
        .expect("spawn 2 MiB generic Cell embedder")
        .join()
        .expect("generic Cell embedder must not overflow");
}

#[test]
fn local_cell_non_string_map_uses_default_evaluator() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let dir = std::env::temp_dir().join(format!(
                "jet_local_cell_int_map_{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("main.jet");
            fs::write(&path, INT_MAP_SOURCE).unwrap();
            let shown = path.to_string_lossy().into_owned();

            jet_jit::reset_jit_trace_for_test();
            let outcome = jet::Interpreter::dev_iteration(&shown, false, false);
            match outcome {
                jet::Interpreter::RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                } => {
                    assert_eq!(exit_code, 0);
                    assert_eq!(stderr, "");
                    assert_eq!(stdout, "2\n");
                }
                jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                    panic!("default tier rejected Cell<[Int: Int]>: {diagnostics:?}")
                }
            }
            assert!(
                !jet_jit::jit_executed_for_test(),
                "Cell<[Int: Int]> must not claim resident-native execution"
            );

            jet_jit::reset_jit_trace_for_test();
            let outcome = jet::Interpreter::dev_iteration(&shown, false, true);
            match outcome {
                jet::Interpreter::RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                } => {
                    assert_eq!(exit_code, 0);
                    assert_eq!(stderr, "");
                    assert_eq!(stdout, "2\n");
                }
                jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                    panic!("interpreter rejected Cell<[Int: Int]>: {diagnostics:?}")
                }
            }
        })
        .expect("spawn 2 MiB non-string map Cell embedder")
        .join()
        .expect("non-string map Cell embedder must not overflow");
}

#[test]
fn local_cell_guards_are_linear_before_codegen() {
    let source = r#"
fn run() {
    cell :: Cell.new(1)
    guard :: cell.guard_read()
    copied :: ~guard
    print(copied.get())
}
"#;
    let diagnostics =
        jet::compile(source).expect_err("Cell guards must be rejected by sema before codegen");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0211"),
        "expected E0211 for explicit Cell guard copy: {diagnostics:?}"
    );
}

#[test]
fn local_cell_guard_storage_boundary_reports_once_per_site() {
    let cases = [
        (
            "struct field",
            r#"
struct Held {
    guard: CellReadGuard<Int>,
}

fn run() {}
"#,
        ),
        (
            "enum payload",
            r#"
enum Held {
    Guard(CellReadGuard<Int>)
}

fn run() {}
"#,
        ),
        (
            "list binding",
            r#"
fn run() {
    cell :: Cell.new(1)
    guard :: cell.guard_read()
    held :: [guard]
}
"#,
        ),
        (
            "lambda capture",
            r#"
fn hold(cell: Cell<Int>) => fn() => Int {
    guard :: cell.guard_read()
    return () => guard.get()
}

fn run() {}
"#,
        ),
        (
            "immediate lambda capture",
            r#"
fn run() {
    cell :: Cell.new(1)
    guard :: cell.guard_read()
    read :: (() => guard.get())()
}
"#,
        ),
        (
            "generic struct temporary",
            r#"
struct Held<T> {
    value: T,
}

fn consume<T>(held: Held<T>) {}

fn run() {
    cell :: Cell.new(1)
    consume(Held<CellReadGuard<Int>>.{ value: cell.guard_read() })
}
"#,
        ),
        (
            "list call argument",
            r#"
fn consume<T>(held: T) {}

fn run() {
    cell :: Cell.new(1)
    consume([cell.guard_read()])
}
"#,
        ),
    ];

    for (case, source) in cases {
        let diagnostics = jet::compile(source)
            .expect_err("unsupported Cell guard storage must be rejected before codegen");
        let count = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0217")
            .count();
        assert_eq!(count, 1, "{case} must report one E0217: {diagnostics:?}");
    }
}
