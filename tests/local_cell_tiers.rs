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
}
"#;

const EXPECTED: &str = "3\n4\n4\n7\n15\n9\n21\n25\nbuilt\nbuilt\n3\n3\n4\n5\n";

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
    let dir = std::env::temp_dir().join(format!(
        "jet_local_cell_parity_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, SOURCE).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("Cell bundle should load");
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "Cell surface must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "Cell surface must stay resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle).expect("Cell surface must compile in resident JIT");

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
