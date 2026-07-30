const SHARED_SOURCE: &str = r#"
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

const COMPOSITE_IF_SOURCE: &str = r#"
struct Work { callback: fn() => Int }
fn both(values: &[Int], work: Work) { values.push(work.callback()) }
fn run() {
    values := [1, 2]
    both(&values, if true -> {
        work :: Work.{ callback: () => values.len() }
        work
    } else -> {
        Work.{ callback: () => 0 }
    })
}
"#;

const COMPOSITE_FALLBACK_SOURCE: &str = r#"
fn both(values: &[Int], callback: fn() => Int) { values.push(callback()) }
fn run() {
    values := [1, 2]
    both(&values, Val(() => values.len()) ?? () => 0)
}
"#;

const LOCAL_CELL_SOURCE: &str = r#"
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

fn compile_on_two_mib_stack(
    source: &'static str,
) -> Result<jet::CompileOutput, Vec<jet::Diagnostics::Diagnostic>> {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || jet::compile(source))
        .expect("spawn the embedder thread")
        .join()
        .expect("the compiler must not overflow the embedder thread")
}

#[test]
fn compile_owns_enough_stack_for_a_two_mib_embedder_thread() {
    compile_on_two_mib_stack(SHARED_SOURCE)
        .expect("the Shared control program must compile");
}

#[test]
fn known_regressions_keep_their_results_on_a_two_mib_embedder_stack() {
    for source in [COMPOSITE_IF_SOURCE, COMPOSITE_FALLBACK_SOURCE] {
        let diagnostics =
            compile_on_two_mib_stack(source).expect_err("the capture conflict must remain E0204");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0204"),
            "{diagnostics:?}"
        );
    }
    compile_on_two_mib_stack(LOCAL_CELL_SOURCE)
        .expect("the full local Cell surface must still compile");
}

fn parenthesized_source(levels: usize) -> String {
    format!(
        "fn nested() => Int {{\n    return {}1{}\n}}\nfn run() {{ print(nested()) }}\n",
        "(".repeat(levels),
        ")".repeat(levels)
    )
}

#[test]
fn public_compile_accepts_depth_256_and_reports_depth_257() {
    jet::compile(&parenthesized_source(254)).expect("source nesting at the limit must compile");

    let diagnostics =
        jet::compile(&parenthesized_source(255)).expect_err("source nesting past the limit must fail");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E1403")
        .expect("source nesting must use the registered diagnostic");
    assert!(diagnostic.what.contains("257 levels deep"));
    assert!(diagnostic.what.contains("limit is 256"));
}
