//! E2-M16 pure evaluation tests (S60, D-PURE1/D-PURE2/D-PURE3).

mod common;

use std::path::Path;
use std::sync::Mutex;

/// Sema's `FuncSig` publishes view provenance through a shared cell; a parsed
/// `Func` still carries the plain map.
fn provenance_cell(
    map: &Option<jet::AST::ViewProvenanceMap>,
) -> jet::AST::ViewProvenanceCell {
    let cell = jet::AST::ViewProvenanceCell::new();
    if let Some(map) = map {
        cell.set(map.clone());
    }
    cell
}

// Serialize all tests that mutate the process-global JET_STORE_DIR to prevent
// concurrent set_var races under cargo's parallel runner.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` with `JET_STORE_DIR` pointed at a fresh `dir`, serializing concurrent
/// calls and restoring the prior value afterward.
fn with_store<T, F: FnOnce() -> T>(dir: &Path, f: F) -> T {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let prev = std::env::var("JET_STORE_DIR").ok();
    std::env::set_var("JET_STORE_DIR", dir);
    let result = f();
    match prev {
        Some(v) => std::env::set_var("JET_STORE_DIR", v),
        None => std::env::remove_var("JET_STORE_DIR"),
    }
    let _ = std::fs::remove_dir_all(dir);
    result
}

/// `pure fn` parses and compiles without error.
#[test]
fn pure_fn_compiles() {
    let src = r#"
fn add(a: Int, b: Int) =[]=> Int {
    return a + b;
}
fn run() {
    print("{add(1, 2)}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "#Pure fn should compile: {:?}", res.err());
}

/// Impure call inside `pure fn` fires E3401.
#[test]
fn pure_fn_impure_call_is_e3401() {
    let src = r#"
fn bad() =[]=> Int {
    print("side effect");
    return 42;
}
fn run() {
    print("{bad()}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "impure call in #Pure fn should fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Card #1543 merge-review finding 1: an `#Impure` body inside a `=[]=>` fn
/// still fires E3401. `#Impure` records/gates the ambient call at run time,
/// it doesn't erase it — a declared-empty effect set can't silently admit
/// one just because it's fenced.
#[test]
fn pure_fn_impure_gate_still_fires_e3401() {
    let src = r#"
fn bad() =[]=> Int {
    #Impure("side effect") {
        print("ambient")
    }
    return 42;
}
fn run() {
    print("{bad()}");
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_err(),
        "#Impure-gated ambient call in a `=[]=>` fn should still fail"
    );
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// Card #1543 merge-review finding 2: a `$ { ... }` comptime block inside a
/// `=[]=>` fn must NOT ALSO trip a run-time-voiced E3401 on top of the real
/// build-time one — it emits no runtime code at all (I3), so the run-time
/// `=[]=>` walk must not descend into it a second time. `print` here is
/// genuinely invalid at compile time (comptime can't touch stdout), so the
/// block's own build-time evaluation correctly reports E3401 once; the bug
/// was the run-time walk piling a second, run-time-voiced E3401 on top.
#[test]
fn pure_fn_comptime_block_is_excluded_from_runtime_check() {
    let src = r#"
fn good() =[]=> Int {
    $ {
        print("build-time only")
    }
    return 42;
}
fn run() {
    print("{good()}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "print() inside `$ {{ }}` is still build-time invalid");
    let diags = res.unwrap_err();
    let e3401s: Vec<_> = diags.iter().filter(|d| d.code == "E3401").collect();
    assert_eq!(
        e3401s.len(),
        1,
        "expected exactly one E3401 (the build-time one), got: {:?}",
        e3401s
    );
    assert!(
        e3401s[0].what.contains("not allowed in comptime code"),
        "the surviving E3401 should be the build-time one, got: {:?}",
        e3401s[0]
    );
}

/// A `pure fn` calling another `pure fn` is fine.
#[test]
fn pure_fn_calling_pure_fn_is_ok() {
    let src = r#"
fn square(n: Int) =[]=> Int {
    return n * n;
}
fn cube(n: Int) =[]=> Int {
    return n * square(n);
}
fn run() {
    print("{cube(3)}");
}
"#;
    let res = jet::compile(src);
    assert!(
        res.is_ok(),
        "pure calling pure should compile: {:?}",
        res.err()
    );
}

/// A public function with an explicit empty effect row compiles.
#[test]
fn pub_pure_fn_compiles() {
    let src = r#"
pub fn double(n: Int) =[]=> Int {
    return n * 2;
}
fn run() {
    print("{double(5)}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "public pure fn should compile: {:?}", res.err());
}

/// `pure fn` calling an impure user-defined function fires E3401.
#[test]
fn pure_fn_calling_impure_user_fn_is_e3401() {
    let src = r#"
fn read_value() => Int {
    print("side effect");
    return 1;
}
fn compute() =[]=> Int {
    return read_value();
}
fn run() {
    print("{compute()}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "#Pure fn calling impure user fn should fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn pure_fn_checks_calls_in_range_bounds() {
    let src = r#"
fn read_bound() => Int {
    print("side effect")
    return 2
}
fn bad() =[]=> Range {
    return 0..read_bound()
}
fn run() {
    print(bad())
}
"#;
    let diags = jet::compile(src).expect_err("an impure range bound must fail");
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got {diags:?}"
    );
}

// ── Transitive impurity traces (check_pure_program_root) ─────────────────────

/// 3-level transitive chain: main → a → b → print must produce E3401 showing
/// the full `main → a → b` path in the why-line.
#[test]
fn transitive_chain_3_levels() {
    use jet::AST;
    use std::collections::HashMap;

    let src = r#"
fn b() {
    print("side effect")
}
fn a() {
    b()
}
fn run() {
    a()
}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let prog = jet::Parser::parse(&toks).expect("parse ok");

    // Build maps.
    let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
    let mut ast_funcs_owned: Vec<(String, AST::Func)> = Vec::new();
    for item in &prog.items {
        if let AST::Item::Func(f) = item {
            funcs_sig.insert(
                f.name.clone(),
                jet::Sema::FuncSig {
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.convention.clone(), p.ty.clone()))
                        .collect(),
                    root_param: f.params.first().is_some_and(|p| p.root),
                    return_type: f.return_type.clone(),
                    return_view_provenance: provenance_cell(&f.return_view_provenance),
                    is_extern: false,
                    is_c_abi: false,
                    c_abi_name: None,
                    foreign_effect_root: None,
                    undo: None,
                    is_unsafe: f.is_unsafe,
                    is_pure: f.is_pure,
                    is_sanitizer: f.is_sanitizer,
                    param_info: f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), p.default.is_some()))
                        .collect(),
                    param_call: f
                        .params
                        .iter()
                        .map(|p| (p.call_label().to_string(), p.zone))
                        .collect(),
                    defaults: f
                        .params
                        .iter()
                        .map(|p| p.default.as_ref().map(|d| *d.clone()))
                        .collect(),
                    param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                    variadic_bounds: f.params.last().and_then(|p| p.variadic_bound_list.clone()),
                    param_view_from_names: f.params.iter().map(|p| p.declared_view_from_names.clone()).collect(),
                    callable_policies: Default::default(),
                    is_must_use: f.is_must_use,
                    is_foreign_thread_safe: false,
                },
            );
            ast_funcs_owned.push((f.name.clone(), f.clone()));
        }
    }
    let ast_funcs: HashMap<String, &AST::Func> = ast_funcs_owned
        .iter()
        .map(|(n, f)| (n.clone(), f))
        .collect();

    let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
    assert!(!diags.is_empty(), "expected E3401 for transitive impurity");
    let d = &diags[0];
    assert_eq!(d.code, "E3401", "should be E3401");
    // The why-line must contain the transitive chain.
    let why = &d.why;
    assert!(
        why.contains("run") && why.contains("a") && why.contains("b"),
        "transitive chain missing from why: {:?}",
        why
    );
    assert!(
        why.contains("→"),
        "chain separator `→` missing from why: {:?}",
        why
    );
}

/// Calls nested in range bounds remain visible to transitive purity analysis.
#[test]
fn transitive_range_bound_is_e3401() {
    use jet::AST;
    use std::collections::HashMap;

    let src = r#"
fn impure_bound() => Int {
    print("oops")
    return 2
}
fn helper() {
    band :: 0..impure_bound()
}
fn run() {
    helper()
}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let prog = jet::Parser::parse(&toks).expect("parse ok");

    let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
    let mut ast_funcs_owned: Vec<(String, AST::Func)> = Vec::new();
    for item in &prog.items {
        if let AST::Item::Func(f) = item {
            funcs_sig.insert(
                f.name.clone(),
                jet::Sema::FuncSig {
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.convention.clone(), p.ty.clone()))
                        .collect(),
                    root_param: f.params.first().is_some_and(|p| p.root),
                    return_type: f.return_type.clone(),
                    return_view_provenance: provenance_cell(&f.return_view_provenance),
                    is_extern: false,
                    is_c_abi: false,
                    c_abi_name: None,
                    foreign_effect_root: None,
                    undo: None,
                    is_unsafe: f.is_unsafe,
                    is_pure: f.is_pure,
                    is_sanitizer: f.is_sanitizer,
                    param_info: f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), p.default.is_some()))
                        .collect(),
                    param_call: f
                        .params
                        .iter()
                        .map(|p| (p.call_label().to_string(), p.zone))
                        .collect(),
                    defaults: f
                        .params
                        .iter()
                        .map(|p| p.default.as_ref().map(|d| *d.clone()))
                        .collect(),
                    param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                    variadic_bounds: f.params.last().and_then(|p| p.variadic_bound_list.clone()),
                    param_view_from_names: f.params.iter().map(|p| p.declared_view_from_names.clone()).collect(),
                    callable_policies: Default::default(),
                    is_must_use: f.is_must_use,
                    is_foreign_thread_safe: false,
                },
            );
            ast_funcs_owned.push((f.name.clone(), f.clone()));
        }
    }
    let ast_funcs: HashMap<String, &AST::Func> = ast_funcs_owned
        .iter()
        .map(|(n, f)| (n.clone(), f))
        .collect();

    let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
    assert!(!diags.is_empty(), "expected E3401");
    let why = &diags[0].why;
    assert!(
        why.contains("run") && why.contains("helper") && why.contains("impure_bound"),
        "chain missing in why: {:?}",
        why
    );
}

/// Pure program with no impure calls must not produce E3401.
#[test]
fn transitive_clean_program_no_error() {
    use jet::AST;
    use std::collections::HashMap;

    let src = r#"
fn square(n: Int) =[]=> Int {
    return n * n
}
fn run() {
    x :: square(5)
}
"#;
    let (toks, _) = jet::Lexer::lex(src);
    let prog = jet::Parser::parse(&toks).expect("parse ok");

    let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
    let mut ast_funcs_owned: Vec<(String, AST::Func)> = Vec::new();
    for item in &prog.items {
        if let AST::Item::Func(f) = item {
            funcs_sig.insert(
                f.name.clone(),
                jet::Sema::FuncSig {
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.convention.clone(), p.ty.clone()))
                        .collect(),
                    root_param: f.params.first().is_some_and(|p| p.root),
                    return_type: f.return_type.clone(),
                    return_view_provenance: provenance_cell(&f.return_view_provenance),
                    is_extern: false,
                    is_c_abi: false,
                    c_abi_name: None,
                    foreign_effect_root: None,
                    undo: None,
                    is_unsafe: f.is_unsafe,
                    is_pure: f.is_pure,
                    is_sanitizer: f.is_sanitizer,
                    param_info: f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), p.default.is_some()))
                        .collect(),
                    param_call: f
                        .params
                        .iter()
                        .map(|p| (p.call_label().to_string(), p.zone))
                        .collect(),
                    defaults: f
                        .params
                        .iter()
                        .map(|p| p.default.as_ref().map(|d| *d.clone()))
                        .collect(),
                    param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                    variadic_bounds: f.params.last().and_then(|p| p.variadic_bound_list.clone()),
                    param_view_from_names: f.params.iter().map(|p| p.declared_view_from_names.clone()).collect(),
                    callable_policies: Default::default(),
                    is_must_use: f.is_must_use,
                    is_foreign_thread_safe: false,
                },
            );
            ast_funcs_owned.push((f.name.clone(), f.clone()));
        }
    }
    let ast_funcs: HashMap<String, &AST::Func> = ast_funcs_owned
        .iter()
        .map(|(n, f)| (n.clone(), f))
        .collect();

    let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
    assert!(
        diags.is_empty(),
        "expected no diagnostics, got: {:?}",
        diags
    );
}

// ── CtValue pretty rendering ──────────────────────────────────────────────────

/// Struct pretty-renders as `Name {\n  field: value,\n}`.
#[test]
fn ctvalue_render_pretty_struct() {
    use jet::CtValue;
    let v = CtValue::Struct {
        type_name: "Point".to_string(),
        fields: vec![
            ("x".to_string(), CtValue::Int(3)),
            ("y".to_string(), CtValue::Int(4)),
        ],
    };
    let rendered = v.render_pretty();
    assert!(rendered.contains("Point"), "missing type name");
    assert!(rendered.contains("x:"), "missing field x");
    assert!(rendered.contains("y:"), "missing field y");
    assert!(rendered.contains("3"), "missing value 3");
    assert!(rendered.contains("4"), "missing value 4");
}

/// List pretty-renders with one item per line.
#[test]
fn ctvalue_render_pretty_list() {
    use jet::CtValue;
    let v = CtValue::List(vec![CtValue::Int(1), CtValue::Int(2), CtValue::Int(3)]);
    let rendered = v.render_pretty();
    assert!(rendered.starts_with('['), "should start with [");
    assert!(rendered.contains("1,"), "missing 1");
    assert!(rendered.contains("2,"), "missing 2");
}

/// Nested struct with list field.
#[test]
fn ctvalue_render_pretty_nested() {
    use jet::CtValue;
    let v = CtValue::Struct {
        type_name: "Report".to_string(),
        fields: vec![
            ("total".to_string(), CtValue::Int(42)),
            (
                "items".to_string(),
                CtValue::List(vec![
                    CtValue::Str("a".to_string()),
                    CtValue::Str("b".to_string()),
                ]),
            ),
        ],
    };
    let rendered = v.render_pretty();
    assert!(rendered.contains("Report"), "missing type name");
    assert!(rendered.contains("total:"), "missing total field");
    assert!(rendered.contains("items:"), "missing items field");
    assert!(rendered.contains("\"a\""), "missing item a");
    assert!(rendered.contains("\"b\""), "missing item b");
}

/// `to_json()` produces compact stable JSON.
#[test]
fn ctvalue_to_json_struct() {
    use jet::CtValue;
    let v = CtValue::Struct {
        type_name: "Point".to_string(),
        fields: vec![
            ("x".to_string(), CtValue::Int(3)),
            ("y".to_string(), CtValue::Int(4)),
        ],
    };
    let json = v.to_json();
    assert_eq!(json, r#"{"x":3,"y":4}"#);
}

/// `to_json()` produces compact stable JSON for lists.
#[test]
fn ctvalue_to_json_list() {
    use jet::CtValue;
    let v = CtValue::List(vec![CtValue::Int(1), CtValue::Int(2)]);
    let json = v.to_json();
    assert_eq!(json, "[1,2]");
}

/// Store generation tracking: list_generations returns an empty list when
/// no generations are recorded (using a temp store dir).
#[test]
fn store_generations_empty() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_empty");
    with_store(&dir, || {
        // Just check it doesn't panic on a fresh store.
        let _ = jet::Store::list_generations();
    });
}

/// Store generation tracking: record_generation writes a record.
#[test]
fn store_record_generation() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_record");
    with_store(&dir, || {
        let gen = jet::Store::record_generation();
        assert!(gen >= 1, "generation should be at least 1");
        let gens = jet::Store::list_generations();
        assert!(
            !gens.is_empty(),
            "should have at least one generation recorded"
        );
    });
}

/// Store rollback: rolling back to a non-existent generation returns Err.
#[test]
fn store_rollback_invalid_gen() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_rollback_inv");
    with_store(&dir, || {
        let result = jet::Store::rollback_to(9999);
        assert!(result.is_err(), "rollback to non-existent gen should fail");
    });
}

// ── Eval sema regression tests ────────────────────────────────────────────────

/// Regression (c54 fix): `jet eval --pure` with a type error must produce a
/// precise type diagnostic (e.g. a binary-operator type mismatch), NOT E0956
/// ("this operation can't run at compile time yet"). Before the fix, the eval
/// path bypassed sema entirely and fell through to the comptime interpreter.
#[test]
fn eval_type_error_gives_precise_diagnostic_not_e0956() {
    // `"string" + 5` is a String/Int type mismatch — sema must catch this.
    let src = r#"fn run() =[]=> Int { return "string" + 5 }"#;
    let diags = jet::check_for_eval(src, "test_eval_type.jet");
    assert!(
        !diags.is_empty(),
        "type error should produce diagnostics, got none"
    );
    assert!(
        diags.iter().all(|d| d.code != "E0956"),
        "must not see E0956 for a type error; got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    // Should be a type-mismatch / operator error, not a comptime limitation.
    let codes: Vec<_> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.iter().any(|c| *c != "E0956"),
        "expected a type/operator error code, got: {:?}",
        codes
    );
}

/// `check_for_eval` passes for a valid typed eval program.
#[test]
fn eval_valid_typed_run_passes_sema() {
    let src = r#"fn run() =[]=> Int { return 2 + 3 }"#;
    let diags = jet::check_for_eval(src, "test_eval_valid.jet");
    assert!(
        diags.is_empty(),
        "`fn run() =[]=> Int` with correct body should pass sema, got: {:?}",
        diags
    );
}

/// `check_for_eval` passes for a normal `fn run() =[]=> ()` program.
#[test]
fn eval_normal_void_run_passes_sema() {
    let src = r#"fn run() =[]=> { }"#;
    let diags = jet::check_for_eval(src, "test_eval_void.jet");
    assert!(
        diags.is_empty(),
        "`fn run() =[]=>` should pass eval sema, got: {:?}",
        diags
    );
}

/// D-REACTCORE1: `#Reactive { … }` parses in statement position.
#[test]
fn parse_reactive_block_stmt() {
    let src = r#"
fn run() {
    #Reactive {
        print(1)
    }
}
"#;
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex: {:?}", lex_diags);
    let prog = jet::Parser::parse(&toks).expect("parse ok");
    let run = prog
        .items
        .iter()
        .find_map(|i| match i {
            jet::AST::Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        })
        .expect("run");
    assert!(
        run.body
            .iter()
            .any(|s| matches!(s, jet::AST::Stmt::Reactive { .. })),
        "expected Stmt::Reactive in run body, got {:?}",
        run.body
    );
}
