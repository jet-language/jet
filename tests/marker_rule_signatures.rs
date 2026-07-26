fn checked(
    source: &str,
    mode: jet::Sema::CompileMode,
) -> (jet::AST::ProgramBundle, Vec<String>) {
    let dir = std::env::temp_dir().join(format!(
        "jet_marker_rule_signatures_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let codes = jet::Sema::check_bundle(&mut bundle, mode)
        .into_iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect();
    (bundle, codes)
}

fn codes(source: &str) -> Vec<String> {
    checked(source, jet::Sema::CompileMode::Check).1
}

#[test]
fn string_and_bool_rule_arguments_use_local_semantic_types() {
    let valid = codes(
        r#"
fn run() {
    category :: "Movement"
    enabled :: true
    #Meta(category: category, tunable: enabled) speed :: 1
}
"#,
    );
    assert!(
        !valid
            .iter()
            .any(|code| matches!(code.as_str(), "E0930" | "E3112" | "L3102")),
        "{valid:?}"
    );

    let wrong = codes(
        r#"
fn run() {
    category :: 42
    enabled :: 1
    #Meta(category: category, tunable: enabled) speed :: 1
}
"#,
    );
    assert_eq!(
        wrong.iter().filter(|code| code.as_str() == "E0930").count(),
        1,
        "{wrong:?}"
    );
    assert!(!wrong.iter().any(|code| matches!(code.as_str(), "E0345" | "E0347")));
}

#[test]
fn int_rule_arguments_use_local_semantic_types() {
    let valid = codes(
        r#"
fn run() {
    deadline :: 42
    #Context(deadline: deadline) {}
}
"#,
    );
    assert!(!valid.iter().any(|code| matches!(code.as_str(), "E0930" | "E0762")));

    let wrong = codes(
        r#"
fn run() {
    deadline :: "soon"
    #Context(deadline: deadline) {}
}
"#,
    );
    assert_eq!(
        wrong.iter().filter(|code| code.as_str() == "E0930").count(),
        1,
        "{wrong:?}"
    );
    assert!(!wrong.iter().any(|code| code == "E0762"));
}

#[test]
fn duration_rule_arguments_use_function_parameter_types() {
    let valid = codes(
        r#"
#[Task, Every(schedule)]
fn tick(schedule: Duration) {}
fn run() {}
"#,
    );
    assert!(
        !valid
            .iter()
            .any(|code| matches!(code.as_str(), "E0930" | "E3112" | "L3102")),
        "{valid:?}"
    );
    assert!(valid.iter().any(|code| code == "E0926"), "{valid:?}");

    let wrong = codes(
        r#"
#[Task, Every(schedule)]
fn tick(schedule: Int) {}
fn run() {}
"#,
    );
    assert_eq!(
        wrong.iter().filter(|code| code.as_str() == "E0930").count(),
        1,
        "{wrong:?}"
    );
    assert!(!wrong.iter().any(|code| code == "E0926"));
}

#[test]
fn block_string_rules_use_local_semantic_types() {
    let valid = codes(
        r#"
fn run() {
    reason :: "audited"
    #Unsafe(reason) {}
    #Impure(reason) {}
    #Nondeterministic(reason) {}
}
"#,
    );
    assert!(
        !valid
            .iter()
            .any(|code| matches!(code.as_str(), "E0930" | "E3112" | "L3102")),
        "{valid:?}"
    );

    let wrong = codes(
        r#"
fn run() {
    reason :: 42
    #Unsafe(reason) {}
    #Impure(reason) {}
    #Nondeterministic(reason) {}
}
"#,
    );
    assert_eq!(
        wrong.iter().filter(|code| code.as_str() == "E0930").count(),
        3,
        "{wrong:?}"
    );
}

#[test]
fn contract_messages_use_function_parameter_types() {
    let valid = codes(
        r#"
#[Pre(value > 0, message), Post(result > 0, message)]
fn positive(value: Int, message: String) -> Int {
    return value
}
fn run() {}
"#,
    );
    assert!(!valid.iter().any(|code| code == "E0930"), "{valid:?}");

    let wrong = codes(
        r#"
#[Pre(value > 0, message), Post(result > 0, message)]
fn positive(value: Int, message: Int) -> Int {
    return value
}
fn run() {}
"#,
    );
    assert_eq!(
        wrong.iter().filter(|code| code.as_str() == "E0930").count(),
        2,
        "{wrong:?}"
    );
}

#[test]
fn static_string_products_resolve_before_consumers() {
    let source = r#"
comptime label = "shared"
comptime invariant = "value >= 0 && value < 4"
comptime page = "index.html"
#Html(page)
#Invariant(invariant)
Tiny :: distinct Int
#Test(label) {}
#Bench(label) {}
fn run() {}
"#;
    let (bundle, diagnostics) = checked(source, jet::Sema::CompileMode::Check);
    assert!(
        !diagnostics.iter().any(|code| code == "E0930"),
        "{diagnostics:?}"
    );
    let module = &bundle.modules[bundle.entry];
    assert_eq!(module.html_path.as_deref(), Some("index.html"));
    let mut saw_test = false;
    let mut saw_bench = false;
    let mut saw_range = false;
    for item in &module.items {
        match item {
            jet::AST::Item::Test(test) => {
                saw_test = test.name.as_deref() == Some("shared")
            }
            jet::AST::Item::Bench(bench) => {
                saw_bench = bench.name.as_deref() == Some("shared")
            }
            jet::AST::Item::Distinct(distinct) if distinct.name == "Tiny" => {
                saw_range = distinct
                    .range
                    .is_some_and(|(low, high, _)| low == 0 && high == 3)
            }
            _ => {}
        }
    }
    assert!(saw_test && saw_bench && saw_range);
}

#[test]
fn static_string_products_report_one_shared_type_error_each() {
    for source in [
        "comptime value = 42\n#Html(value)\nfn run() {}",
        "comptime value = 42\n#Invariant(value)\nTiny :: distinct Int\nfn run() {}",
        "comptime value = 42\n#Test(value) {}\nfn run() {}",
        "comptime value = 42\n#Bench(value) {}\nfn run() {}",
    ] {
        let diagnostics = codes(source);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|code| code.as_str() == "E0930")
                .count(),
            1,
            "{diagnostics:?}\n{source}"
        );
    }
}

#[test]
fn static_string_products_reject_nonstatic_expressions_once() {
    for source in [
        "#Html(runtime_name)\nfn run() {}",
        "#Invariant(runtime_name)\nTiny :: distinct Int\nfn run() {}",
        "#Test(runtime_name) {}\nfn run() {}",
        "#Bench(runtime_name) {}\nfn run() {}",
    ] {
        let diagnostics = codes(source);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|code| code.as_str() == "E0930")
                .count(),
            1,
            "{diagnostics:?}\n{source}"
        );
    }
}

#[test]
fn resolved_invariant_text_keeps_domain_validation() {
    let diagnostics = codes(
        r#"
comptime invariant = "value != 3"
#Invariant(invariant)
Tiny :: distinct Int
fn run() {}
"#,
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|code| code.as_str() == "E0003")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert!(!diagnostics.iter().any(|code| code == "E0930"));
}

#[test]
fn duplicate_html_markers_still_fail_before_resolution() {
    let source = r#"
comptime first = "first.html"
comptime second = "second.html"
#Html(first)
#Html(second)
fn run() {}
"#;
    let dir = std::env::temp_dir().join(format!(
        "jet_marker_rule_signatures_duplicate_html_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, source).unwrap();
    let diagnostics = jet::Loader::load_entry(path.to_str().unwrap())
        .expect_err("duplicate Html markers must fail during loading");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0003")
            .count(),
        1,
        "{diagnostics:?}"
    );
}

#[test]
fn generic_instances_materialize_distinct_test_and_bench_names() {
    let (bundle, diagnostics) = checked(
        r#"
module suite<label: String> {
    #Test(label) {}
    #Bench(label) {}
}
module first = suite<"case">
module second = suite<"other">
fn run() {}
"#,
        jet::Sema::CompileMode::Check,
    );
    assert!(
        !diagnostics
            .iter()
            .any(|code| matches!(code.as_str(), "E0105" | "E0930")),
        "{diagnostics:?}"
    );
    let mut names = bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            jet::AST::Item::Test(test) => test.name.clone(),
            jet::AST::Item::Bench(bench) => bench.name.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["first_case", "first_case", "second_other", "second_other"]
    );
}

#[test]
fn resolved_test_names_keep_duplicate_identity() {
    let diagnostics = codes(
        r#"
comptime name = "same"
#Test(name) {}
#Test("same") {}
fn run() {}
"#,
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|code| code.as_str() == "E0105")
            .count(),
        1,
        "{diagnostics:?}"
    );
}

#[test]
fn formatter_preserves_static_rule_expressions() {
    let source = "comptime name = \"case\"\n#Test(name) {}\n#Bench(name) {}\n#Html(name)\nfn run() {}\n";
    let formatted = jet::format_source(source).expect("static rule expressions should format");
    assert!(formatted.contains("#Test(name)"), "{formatted}");
    assert!(formatted.contains("#Bench(name)"), "{formatted}");
    assert!(formatted.contains("#Html(name)"), "{formatted}");
}
