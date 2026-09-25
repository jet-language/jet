use jet::AST::CtValue;
use jet::Diagnostics::Span;

fn ambient_parse(source: &str) -> CtValue {
    let result = jet::Comptime::try_ambient_core_call(
        "core.compiler",
        "parse",
        vec![CtValue::Str(source.to_string())],
        Span { start: 0, end: source.len() },
    ).expect("the root compiler boundary must install its Core evaluator")
        .expect("parser diagnostics are returned as compiler values");
    let CtValue::Present(tree) = result else {
        panic!("expected a compiler syntax tree, got {result:?}");
    };
    *tree
}

fn field<'a>(value: &'a CtValue, name: &str) -> &'a CtValue {
    let CtValue::Struct { fields, .. } = value else {
        panic!("expected compiler record, got {value:?}");
    };
    fields.iter().find_map(|(key, value)| (key == name).then_some(value))
        .unwrap_or_else(|| panic!("missing compiler field {name}"))
}

fn assert_parsed_function(tree: &CtValue, expected: &str) {
    let CtValue::List(items) = field(tree, "items") else {
        panic!("compiler syntax tree must expose its parsed items");
    };
    assert!(items.iter().any(|item| {
        matches!(field(item, "kind"), CtValue::Str(kind) if kind == "function")
            && matches!(field(item, "name"), CtValue::Present(name)
                if matches!(name.as_ref(), CtValue::Str(name) if name == expected))
    }), "the ambient call must parse function {expected}");
}

fn assert_no_compiler_callback() {
    assert!(jet::Comptime::try_ambient_core_call(
        "core.compiler", "parse", vec![CtValue::Str("fn run() {}".to_string())],
        Span { start: 0, end: 11 },
    ).is_none(), "compiler authority must not escape its scoped entry");
}

#[test]
fn root_boundary_runs_real_compiler_queries_on_one_scoped_worker() {
    assert_no_compiler_callback();
    let caller = std::thread::current().id();
    jet::run_compiler_work(|| {
        assert!(jet_foundation::CompilerStack::on_compiler_worker());
        let worker = std::thread::current().id();
        assert_ne!(worker, caller);
        let tree = ambient_parse("fn retained_host_probe() {}\n");
        assert_parsed_function(&tree, "retained_host_probe");
        jet::run_compiler_work(|| {
            assert_eq!(std::thread::current().id(), worker);
            let invalid = ambient_parse("fn run( {\n");
            let CtValue::List(diagnostics) = field(&invalid, "diagnostics") else {
                panic!("compiler syntax tree must expose diagnostics");
            };
            assert!(diagnostics.iter().any(|diagnostic| {
                matches!(field(diagnostic, "code"), CtValue::Str(code) if code == "E0003")
            }), "malformed syntax must keep the parser's diagnostic across the host seam");
        });
        let panic = std::panic::catch_unwind(|| {
            jet::run_compiler_work(|| std::panic::panic_any(0x3581_u32));
        }).expect_err("nested compiler panic must reach the active worker");
        assert_eq!(panic.downcast_ref::<u32>(), Some(&0x3581));
        assert_eq!(std::thread::current().id(), worker);
        assert_parsed_function(&ambient_parse("fn still_scoped() {}\n"), "still_scoped");
    });
    assert_no_compiler_callback();
}

#[test]
fn root_boundary_preserves_panic_payload_and_releases_compiler_authority() {
    assert_no_compiler_callback();
    let panic = std::panic::catch_unwind(|| {
        jet::run_compiler_work(|| {
            let tree = ambient_parse("fn before_unwind() {}\n");
            assert_parsed_function(&tree, "before_unwind");
            std::panic::panic_any(0x218_u32);
        });
    }).expect_err("the retained Rust worker must propagate its panic");
    assert_eq!(panic.downcast_ref::<u32>(), Some(&0x218));
    assert_no_compiler_callback();
    assert!(!jet_foundation::CompilerStack::on_compiler_worker());
}
