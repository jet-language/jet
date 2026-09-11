mod common;

const SHARED_SOURCE: &str = r#"
struct Counter { value: Int }
fn run() {
    counter := shared Counter{ value: 0 }
    handle :: task {
        counter.value += 1
    }
    handle.join() ?? panic("task failed")
    print(counter.value)
}
"#;

const COMPOSITE_IF_SOURCE: &str = r#"
struct Work { callback: fn() Int }
fn both(values: &[Int], work: Work) { values.push(work.callback()) }
fn run() {
    values := [1, 2]
    both(&values, if true -> {
        work :: Work{ callback: () -> values.len() }
        work
    } else -> {
        Work{ callback: () -> 0 }
    })
}
"#;

const COMPOSITE_FALLBACK_SOURCE: &str = r#"
fn both(values: &[Int], callback: fn() Int) { values.push(callback()) }
fn run() {
    values := [1, 2]
    both(&values, Val(() -> values.len()) ?? () -> 0)
}
"#;

const LOCAL_CELL_SOURCE: &str = r#"
struct Pair {
    left: Int,
    right: Int,
}

struct ValueCache {
    value: Cell<?String>,
}

struct LineCache {
    value: Cell<?[Int]>,
}

fn mapped_read(cell: Cell<Pair>) {
    left :: cell.guard_read().map(pair -> pair.left)
    print(left.get())
}

fn split_read(cell: Cell<Pair>) {
    (left, right) :: cell.guard_read().split(
        pair -> pair.left,
        pair -> pair.right
    )
    print(left.get() + right.get())
}

fn mapped_edit(cell: Cell<Pair>) {
    left :: cell.guard_edit().map(pair -> pair.left)
    left.set(9)
}

fn split_edit(cell: Cell<Pair>) {
    (left, right) :: cell.guard_edit().split(
        pair -> pair.left,
        pair -> pair.right
    )
    left.set(10)
    right.set(11)
}

fn make_edit_guards(cell: Cell<Pair>) (
    first: CellEditGuard<Int>,
    second: CellEditGuard<Int>
) -> {
    return cell.guard_edit().split(
        pair -> pair.left,
        pair -> pair.right
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
    cell :: Cell.new(Pair{ left: 1, right: 2 })
    print(cell.read(pair -> pair.left + pair.right))
    cell.edit(pair -> pair.left += 3)
    print(cell.get().left)
    old :: cell.replace(Pair{ left: 5, right: 6 })
    print(old.left)
    cell.set(Pair{ left: 7, right: 8 })
    mapped_read(cell)
    split_read(cell)
    mapped_edit(cell)
    print(cell.get().left)
    split_edit(cell)
    print(cell.get().left + cell.get().right)
    edit_returned_split(cell)
    print(cell.get().left + cell.get().right)

    cache :: ValueCache{ value: Cell.new(None) }
    print(cache.value.get_or_set(() -> "built"))
    print(cache.value.get_or_set(() -> "unused"))

    lines :: LineCache{ value: Cell.new(None) }
    print(lines.value.get_or_set(() -> [0, 8, 15]).len())
    print(lines.value.get_or_set(() -> [99]).len())

    early :: Cell.new(1)
    edit_then_return(early)
    print(early.get())
    early.edit(value -> value += 1)
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
    compile_on_two_mib_stack(SHARED_SOURCE).expect("the Shared control program must compile");
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

const JIT_ENTRY_SOURCE: &str = r#"
fn twice(n: Int) Int -> {
    return n + n
}
fn run() {
    print(twice(21))
}
"#;

const PACKAGE_EDITION_PROGRAM: &str = r#"
use core.data as data

#Codable
struct Sale {
    month: String
    value: Float
}

fn run() {
    rows :: data.csv<Sale>("month,value\nJan,4.0\nFeb,6.0") ?? panic("csv")
    groups :: data.query(rows)
        .group_by(sale -> sale.month)
        .mean(sale -> sale.value)
        .collect() ?? panic("groups")
    if groups.len() == 2 {
        print("checked")
    }
    print("edition-sentinel")
}
"#;

const PACKAGE_EDITION_MANIFEST: &str =
    "name: \"tir_eval_edition\"\nversion: \"0.1.0\"\nedition: \"2027\"\nauthority: { holds: { allow: [IO, Mem.Alloc, Panic] } }\n";

#[test]
fn package_edition_survives_tir_eval_worker() {
    let dir = common::unique_tmp("jet_compiler_stack_package_edition");
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    std::fs::write(dir.join("package.jet"), PACKAGE_EDITION_MANIFEST)
        .expect("write the package manifest");
    let file = dir.join("main.jet");
    std::fs::write(&file, PACKAGE_EDITION_PROGRAM).expect("write the fixture");
    let path = file.to_string_lossy().into_owned();

    for (tier, outcome) in [
        (
            "forced interpreter",
            jet::Interpreter::run_interpreter_once_with_args(&path, &[]),
        ),
        (
            "default JIT",
            jet::Interpreter::run_jit_once_with_args(&path, &[]),
        ),
    ] {
        match outcome {
            jet::Interpreter::RunOutcome::Ran {
                exit_code,
                stdout,
                stderr,
            } => {
                assert_eq!(exit_code, 0, "{tier} must run: {stderr}");
                assert!(stderr.is_empty(), "{tier} emitted diagnostics: {stderr}");
                assert_eq!(
                    stdout, "checked\nedition-sentinel\n",
                    "{tier} lost the package edition"
                );
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                panic!("{tier} rejected the edition fixture: {diagnostics:?}");
            }
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// An embedder holding a checked bundle reaches the JIT without ever touching
/// the driver's compile entries, so the Cranelift backend has to own the same
/// sized stack they own: MIR lowering and the whole-program interpreter on the
/// deopt route are the same unbounded-depth recursive descent, and either tier
/// alone exhausts a 2 MiB embedder thread.
#[test]
fn the_jit_backend_owns_enough_stack_for_a_two_mib_embedder_thread() {
    let dir = common::unique_tmp("jet_compiler_stack_jit_entry");
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    let file = dir.join("main.jet");
    std::fs::write(&file, JIT_ENTRY_SOURCE).expect("write the fixture");
    let path = file.to_string_lossy().into_owned();

    let outcome = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let bundle = jet::run_compiler_work(move || {
                let mut bundle = jet::Loader::load_entry(&path).expect("the fixture should load");
                let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
                assert!(
                    !diagnostics
                        .iter()
                        .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
                    "{diagnostics:?}"
                );
                bundle
            });
            let mut backend = jet_jit::CraneliftBackend::new();
            let policy = common::development_policy();
            common::run_cranelift_bundle(&mut backend, &bundle, false, &policy)
        })
        .expect("spawn the embedder thread")
        .join()
        .expect("the JIT backend must not overflow the embedder thread");

    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, "42\n"),
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("the fixture must run on one of the two tiers: {diagnostics:?}")
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The boundary is installed at both the driver's entries and the JIT's, so it
/// has to be re-entrant in both directions or a nested entry would spawn a
/// worker inside a worker. The thread id is the proof: an inner entry that
/// stays on the outer worker's thread did not spawn.
#[test]
fn the_compiler_boundary_never_nests_a_second_worker() {
    assert!(
        !jet_foundation::CompilerStack::on_compiler_worker(),
        "a test thread is not a compiler worker"
    );

    let (worker, driver_inside_driver, jit_inside_driver) = jet::run_compiler_work(|| {
        assert!(
            jet_foundation::CompilerStack::on_compiler_worker(),
            "the flag must be set on the worker, not on the thread that spawned it"
        );
        (
            std::thread::current().id(),
            jet::run_compiler_work(|| std::thread::current().id()),
            jet_jit::on_compiler_stack(|| std::thread::current().id()),
        )
    });
    assert_eq!(
        worker, driver_inside_driver,
        "a nested driver entry must reuse the active worker"
    );
    assert_eq!(
        worker, jit_inside_driver,
        "a JIT entry inside a driver entry must reuse the active worker"
    );

    let (worker, driver_inside_jit) = jet_jit::on_compiler_stack(|| {
        (
            std::thread::current().id(),
            jet::run_compiler_work(|| std::thread::current().id()),
        )
    });
    assert_eq!(
        worker, driver_inside_jit,
        "a driver entry inside a JIT entry must reuse the active worker"
    );

    assert_ne!(
        worker,
        std::thread::current().id(),
        "an outermost entry must actually leave the caller's thread"
    );
    assert!(
        !jet_foundation::CompilerStack::on_compiler_worker(),
        "the flag must not leak back onto the caller"
    );
}

fn parenthesized_source(levels: usize) -> String {
    format!(
        "fn nested() Int -> {{\n    return {}1{}\n}}\nfn run() {{ print(nested()) }}\n",
        "(".repeat(levels),
        ")".repeat(levels)
    )
}

#[test]
fn public_compile_accepts_depth_256_and_reports_depth_257() {
    jet::compile(&parenthesized_source(254)).expect("source nesting at the limit must compile");

    let diagnostics = jet::compile(&parenthesized_source(255))
        .expect_err("source nesting past the limit must fail");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E1403")
        .expect("source nesting must use the registered diagnostic");
    assert!(diagnostic.what.contains("257 levels deep"));
    assert!(diagnostic.what.contains("limit is 256"));
}

