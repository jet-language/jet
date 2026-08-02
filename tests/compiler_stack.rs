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

fn tir_func(
    name: &str,
    body: Vec<jet::Codegen::TIR::TStmt>,
    source_span: jet::Diagnostics::Span,
) -> jet::Codegen::TIR::TFunc {
    jet::Codegen::TIR::TFunc {
        name: name.to_string(),
        source_span,
        params: Vec::new(),
        web_param_reconstructions: Vec::new(),
        ret: None,
        gc_return: false,
        return_view_provenance: None,
        generics: String::new(),
        clone_types: Vec::new(),
        is_main: name == "run",
        line: 1,
        is_unsafe: false,
        is_pure: true,
        is_reactive: false,
        reactive_upgrades: Vec::new(),
        is_inline: false,
        is_inline_always: false,
        kernel_proof: None,
        body,
        kind: jet::Codegen::TIR::TFuncKind::TopLevel,
    }
}

fn nested_tir_expr(nodes: usize) -> jet::Codegen::TIR::TExpr {
    use jet::Codegen::TIR::{TExpr, TExprKind};

    let mut expr = TExpr {
        ty: jet::AST::Type::Named("Unit".to_string()),
        kind: TExprKind::Unit,
    };
    for _ in 1..nodes {
        expr = TExpr {
            ty: jet::AST::Type::Named("Unit".to_string()),
            kind: TExprKind::Clone(Box::new(expr)),
        };
    }
    expr
}

fn nested_tir_program(
    nodes: usize,
    source_span: jet::Diagnostics::Span,
) -> jet::Codegen::TIR::JitProgram {
    use jet::Codegen::TIR::{JitProgram, TStmt};

    JitProgram {
        source_file: "nested-tir.jet".to_string(),
        entry: "run".to_string(),
        instance_provenance: Vec::new(),
        funcs: vec![tir_func(
            "run",
            vec![TStmt::ExprStmt(nested_tir_expr(nodes))],
            source_span,
        )],
        spawn_lambdas: Vec::new(),
        struct_fields: std::collections::HashMap::new(),
        struct_field_types: std::collections::HashMap::new(),
        struct_type_params: std::collections::HashMap::new(),
        enum_variants: std::collections::HashMap::new(),
        enum_variant_payload_types: std::collections::HashMap::new(),
        canonical_deopt: std::collections::HashSet::new(),
        canonical_calls: std::collections::HashSet::new(),
        int_constants: std::collections::HashMap::new(),
        constants: std::collections::HashMap::new(),
        distinct_bases: std::collections::HashMap::new(),
        distinct_ranges: std::collections::HashMap::new(),
        codec_migrations: std::collections::HashMap::new(),
        trait_method_owners: std::collections::HashMap::new(),
        iterable_item_types: std::collections::HashMap::new(),
    }
}

fn run_tir_program(
    program: jet::Codegen::TIR::JitProgram,
) -> Result<jet::Comptime::CtValue, jet::Diagnostics::Diagnostic> {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            jet::Codegen::TIR::run_program(
                &program,
                std::path::Path::new("."),
                &mut jet::Comptime::DevSink::new(),
                std::collections::HashMap::new(),
                &std::collections::HashMap::new(),
                false,
            )
        })
        .expect("spawn TIR boundary evaluator")
        .join()
        .expect("TIR boundary evaluator must not panic")
}

fn run_nested_tir(
    nodes: usize,
    source_span: jet::Diagnostics::Span,
) -> Result<jet::Comptime::CtValue, jet::Diagnostics::Diagnostic> {
    run_tir_program(nested_tir_program(nodes, source_span))
}

#[test]
fn canonical_tir_evaluator_accepts_depth_256_and_renders_depth_257() {
    let source = "fn run() {\n    value\n}\n";
    let span = jet::Diagnostics::Span::new(0, source.len());

    run_nested_tir(255, span).expect("one statement plus 255 expressions is depth 256");

    let diagnostic =
        run_nested_tir(256, span).expect_err("one statement plus 256 expressions is depth 257");
    assert_eq!(diagnostic.code, "E1403");
    assert_eq!(diagnostic.span, Some(span));
    let rendered = jet::Diagnostics::render_all("nested-tir.jet", source, &[diagnostic]);
    assert!(rendered.contains("nested-tir.jet:1:"), "{rendered}");
    assert!(rendered.contains("fn run()"), "{rendered}");
}

#[test]
fn tir_function_entry_resets_structural_depth() {
    use jet::Codegen::TIR::{TExpr, TExprKind, TStmt};

    let source = "fn helper() {}\nfn run() {}\n";
    let span = jet::Diagnostics::Span::new(0, source.len());
    let mut program = nested_tir_program(255, span);
    program.funcs[0].name = "helper".to_string();
    program.funcs[0].is_main = false;
    program.funcs.push(tir_func(
        "run",
        vec![TStmt::ExprStmt(TExpr {
            ty: jet::AST::Type::Named("Unit".to_string()),
            kind: TExprKind::Call {
                name: "helper".to_string(),
                args: Vec::new(),
            },
        })],
        span,
    ));

    run_tir_program(program).expect("runtime call depth must not count as source nesting");
}
