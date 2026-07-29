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

fn parse_codes(source: &str) -> Vec<String> {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    match jet::Parser::parse(&tokens) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect(),
    }
}

#[test]
fn signed_auto_derive_markers_parse_on_types() {
    for source in [
        "#!Printable\nstruct Quiet { value: Int }\nfn run() {}",
        "#[!Debug, !Equatable, Printable]\nstruct Mixed { value: Int }\nfn run() {}",
    ] {
        let diagnostics = parse_codes(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}\n{source}");
    }
}

#[test]
fn negative_sign_is_reserved_for_auto_derive_traits() {
    for source in [
        "#!Pure\nfn run() {}",
        "#[!Inline]\nfn run() {}",
        "#!Comparable\nstruct Bad { value: Int }\nfn run() {}",
    ] {
        let diagnostics = parse_codes(source);
        assert_eq!(diagnostics, vec!["E0931"], "{diagnostics:?}\n{source}");
    }
}

#[test]
fn field_and_variant_sites_reject_every_non_applicable_example() {
    for source in [
        "#[Task, Static] struct Bad { value: Int }\nfn run() {}",
        "enum Bad { #Comparable Value }\nfn run() {}",
        "enum Bad { #Skip Value }\nfn run() {}",
    ] {
        let diagnostics = parse_codes(source);
        assert_eq!(
            diagnostics.iter().filter(|code| *code == "E0355").count(),
            1,
            "{diagnostics:?}\n{source}"
        );
    }
}

#[test]
fn method_markers_share_the_ordered_registry_collector() {
    let source = r#"
state Door { Ready }
struct Door {
    #[Inline, State(Ready)]
    fn open(self) {}
}
fn run() {}
"#;
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("method marker list should parse");
    let jet::AST::Item::Struct(door) = &program.items[1] else {
        panic!("Door struct")
    };
    assert!(door.methods[0].is_inline);
    assert_eq!(
        door.methods[0].state_requires.as_ref().map(|state| state.0.as_str()),
        Some("Ready")
    );

    let wrong = parse_codes("struct Bad { #Task fn work(self) {} }\nfn run() {}");
    assert_eq!(wrong.iter().filter(|code| *code == "E0925").count(), 1);
}

#[test]
fn parser_binds_the_authoritative_declaration_site_matrix() {
    let fixtures = [
        (
            "#allow(float_money) price: Float",
            "struct Money { #allow(float_money) price: Float }\nfn run() {}",
            jet::Policy::RuleSite::Field,
        ),
        (
            "#Rename on enum variant",
            "enum Status { #Rename(\"ready\") Ready }\nfn run() {}",
            jet::Policy::RuleSite::Variant,
        ),
        (
            "#Inline on method",
            "struct Box { #Inline fn open(self) {} }\nfn run() {}",
            jet::Policy::RuleSite::Method,
        ),
        (
            "#HTML on file",
            "#HTML(\"index.html\")\nfn run() {}",
            jet::Policy::RuleSite::File,
        ),
        (
            "#Test declaration",
            "#Test(\"works\") {}\nfn run() {}",
            jet::Policy::RuleSite::Test,
        ),
        (
            "#Bench declaration",
            "#Bench(\"fast\") {}\nfn run() {}",
            jet::Policy::RuleSite::Bench,
        ),
        (
            "#Task on function",
            "#Task fn work() {}\nfn run() {}",
            jet::Policy::RuleSite::Function,
        ),
    ];
    for (label, source, expected_site) in fixtures {
        let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{label}: {lexer_diagnostics:?}");
        let program = jet::Parser::parse(&tokens)
            .unwrap_or_else(|diagnostics| panic!("{label}: {diagnostics:?}"));
        assert!(
            program
                .rule_facts
                .iter()
                .any(|application| application.site == Some(expected_site)),
            "{label}: {:?}",
            program.rule_facts
        );
    }

    let (tokens, lexer_diagnostics) =
        jet::Lexer::lex("#Policy(no_alloc)\nfn run() {}");
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens)
        .unwrap_or_else(|diagnostics| panic!("#Policy module declaration: {diagnostics:?}"));
    assert!(
        program
            .policy_declarations
            .iter()
            .any(|declaration| declaration.scope == jet::Policy::PolicyScope::Module),
        "#Policy module declaration: {:?}",
        program.policy_declarations
    );
}

#[test]
fn adjacent_method_markers_keep_the_shared_e0999_rewrite() {
    let source =
        "state Door { Ready }\nstruct Door { #Inline #State(Ready) fn open(self) {} }\nfn run() {}";
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
    let (_, diagnostics) =
        jet::Parser::parse_for_check(&tokens).expect("adjacent method markers recover");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0999")
        .expect("shared adjacent-marker diagnostic");
    assert_eq!(
        diagnostic.edit.as_ref().map(|edit| edit.new_text.as_str()),
        Some("#[Inline, State(Ready)]")
    );
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
fn positive(value: Int, message: String) => Int {
    return value
}
fn run() {}
"#,
    );
    assert!(!valid.iter().any(|code| code == "E0930"), "{valid:?}");

    let wrong = codes(
        r#"
#[Pre(value > 0, message), Post(result > 0, message)]
fn positive(value: Int, message: Int) => Int {
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
#HTML(page)
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
        "comptime value = 42\n#HTML(value)\nfn run() {}",
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
fn static_type_and_field_strings_use_the_same_signature_gate() {
    let valid = codes(
        r#"
comptime tag_name = "kind"
comptime field_name = "identifier"
comptime variant_name = "ready"
#[Codable, Tag(tag_name)]
enum Event { #Rename(variant_name) Ready }
#Codable
struct Row { #Rename(field_name) id: Int }
fn run() {}
"#,
    );
    assert!(!valid.iter().any(|code| code == "E0930"), "{valid:?}");

    for source in [
        "comptime value = 42\n#[Codable, Tag(value)] enum Event { Ready }\nfn run() {}",
        "comptime value = 42\n#Codable struct Row { #Rename(value) id: Int }\nfn run() {}",
        "comptime value = 42\n#Codable enum Event { #Rename(value) Ready }\nfn run() {}",
    ] {
        let diagnostics = codes(source);
        assert_eq!(
            diagnostics.iter().filter(|code| *code == "E0930").count(),
            1,
            "{diagnostics:?}\n{source}"
        );
        assert!(
            !diagnostics
                .iter()
                .any(|code| matches!(code.as_str(), "E2407" | "E2409")),
            "{diagnostics:?}\n{source}"
        );
    }
}

#[test]
fn static_string_products_reject_nonstatic_expressions_once() {
    for source in [
        "#HTML(runtime_name)\nfn run() {}",
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
#HTML(first)
#HTML(second)
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
        .expect_err("duplicate HTML markers must fail during loading");
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
    let source = "comptime name = \"case\"\n#Test(name) {}\n#Bench(name) {}\n#HTML(name)\nfn run() {}\n";
    let formatted = jet::format_source(source).expect("static rule expressions should format");
    assert!(formatted.contains("#Test(name)"), "{formatted}");
    assert!(formatted.contains("#Bench(name)"), "{formatted}");
    assert!(formatted.contains("#HTML(name)"), "{formatted}");
}
