//! D-WASM1=A (c123 M1): web JS/WASM partition sema.

fn codes(src: &str) -> Vec<String> {
    jet::compile(src)
        .err()
        .unwrap()
        .into_iter()
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn wasm_calling_js_is_cross_partition() {
    let src = include_str!("ui/web_cross_partition.jet");
    let c = codes(src);
    assert!(
        c.iter().any(|x| x == "E-WEB-CROSS-PARTITION"),
        "expected E-WEB-CROSS-PARTITION, got {c:?}"
    );
}

#[test]
fn wasm_export_struct_param_is_abi_error() {
    let src = include_str!("ui/web_abi_type.jet");
    let c = codes(src);
    assert!(
        c.iter().any(|x| x == "E-WEB-ABI-TYPE"),
        "expected E-WEB-ABI-TYPE, got {c:?}"
    );
}

#[test]
fn browser_effect_infers_js_bucket_without_marker() {
    let src = r#"
use core.ui as ui

fn dom_fn() {
    _b :: ui.null_backend()
}

#Target(Wasm)
fn compute() -> Int {
    return 1
}

fn run() {
    print("{compute()}")
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "main path should compile: {:?}", res.err());
}

#[test]
fn wasm_pin_with_browser_effect_is_target_browser_error() {
    let src = include_str!("ui/web_target_browser.jet");
    let c = codes(src);
    assert!(
        c.iter().any(|x| x == "E-WEB-TARGET-BROWSER"),
        "expected E-WEB-TARGET-BROWSER, got {c:?}"
    );
}

#[test]
fn codable_struct_wasm_export_is_abi_safe() {
    let src = include_str!("ui/web_abi_codable.jet");
    let res = jet::compile_web_with_path(src, "tests/ui/web_abi_codable.jet");
    assert!(
        res.is_ok(),
        "Codable struct export should be ABI-safe: {:?}",
        res.err()
    );
}

#[test]
fn ordinary_wasm_struct_field_does_not_gain_export_boundary_support() {
    let src = r#"struct Point { x: Int, y: Int }

#Target(Wasm)
fn read_x(p: Point) -> Int { return p.x }

fn run() {}
"#;
    let diags = jet::compile_web_with_path(src, "tests/fixtures/web_internal_struct_field.jet")
        .expect_err("ordinary Wasm fields must remain outside the supported subset");
    assert_eq!(
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E-WEB-TIR-UNSUPPORTED"]
    );
}

#[test]
fn web_partition_report_generated_for_web_compile() {
    let src = include_str!("../examples/features/web/web_compute.jet");
    let out = jet::compile_web_with_path(src, "examples/features/web/web_compute.jet")
        .expect("web compile should succeed");
    let report = out
        .web_partition_report
        .expect("web compile should set partition report");
    assert!(
        report.contains("Web partition report"),
        "unexpected report:\n{report}"
    );
    assert!(report.contains("compute"), "report should list compute fn");
}

#[test]
fn browser_effect_partition_metadata() {
    let src = r#"
use core.ui as ui

fn dom_fn() {
    _b :: ui.null_backend()
}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let mut prog = jet::Parser::parse(&toks).unwrap();
    let mut bundle = jet::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        ffi_callback_fns: std::collections::HashSet::new(),
        modules: vec![jet::AST::LoadedModule {
            path: std::path::PathBuf::from("t.jet"),
            display: "t.jet".to_string(),
            source: src.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            block_spans: std::mem::take(&mut prog.block_spans),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: std::mem::take(&mut prog.policy_declarations),
        }],
        parse_teaching: vec![],
        used_core: Default::default(),
        cffi: Default::default(),
        comptime_inputs: vec![],
        import_targets: Default::default(),
        layer_ceiling: None,
        inferred_layer: jet::Syntax::RuntimeLayer::Core,
        web_partitions: Default::default(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: Default::default(),
        active_os: jet::Syntax::OsTarget::host(),
    };
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert_eq!(
        bundle.web_partitions.get("dom_fn"),
        Some(&jet::Syntax::WebBucket::Js)
    );
}
