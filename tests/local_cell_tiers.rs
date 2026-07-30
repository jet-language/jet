#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, compile, have_rustc, run_default_multi};

const SOURCE: &str = r#"
struct Pair {
    left: Int,
    right: Int,
}

struct Cache {
    value: Cell<String?>,
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

    cache :: Cache.{ value: Cell.new(None) }
    print(cache.value.get_or_set(() => "built"))
    print(cache.value.get_or_set(() => "unused"))
}
"#;

const EXPECTED: &str = "3\n4\n4\n7\n15\n9\n21\nbuilt\nbuilt\n";

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
    let (code, stdout, stderr) =
        run_default_multi("local_cell_default", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, EXPECTED);
}
