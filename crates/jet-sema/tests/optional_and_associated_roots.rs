use jet_sema::AST::{CFfi, ImportKind, LoadedModule, ProgramBundle};
use jet_sema::Diagnostics::Severity;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn check_sources(sources: &[(&str, &str, &str, &[(&str, usize)])]) -> Vec<String> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(jet_codegen::Codegen::MIREval::install_mir_bridge);
    let mut modules = Vec::new();
    let mut name_ledger = jet_sema::AST::NameLedger::default();
    for (index, (path, alias, source, targets)) in sources.iter().enumerate() {
        let (tokens, diagnostics) = if path.starts_with("<corelib>/") {
            Lexer::lex_generated(source)
        } else {
            Lexer::lex(source)
        };
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut program = Parser::parse(&tokens).expect("regression source parses");
        for (import_alias, target) in *targets {
            let import = program.imports.iter().find(|import|
                import.import_alias() == *import_alias
                    && !matches!(import.kind, ImportKind::Unqualified { .. })).unwrap();
            name_ledger.record_import_target(index, import.span, *target);
        }
        modules.push(LoadedModule {
            path: PathBuf::from(path), display: (*path).into(), alias: (*alias).into(),
            imports: std::mem::take(&mut program.imports), items: program.items,
            script_body: program.script_body, source: (*source).into(),
            core_identifier_demand: Lexer::core_identifier_demand(&tokens),
            block_spans: program.block_spans, web_target_ceiling: program.web_target_ceiling,
            no_prelude: program.no_prelude, default_target: program.default_target,
            html_path: program.html_path, policy_declarations: program.policy_declarations,
            user_policy_declarations: program.user_policy_declarations, rule_facts: program.rule_facts,
        });
    }
    let mut bundle = ProgramBundle {
        entry: sources.len() - 1, project_root: PathBuf::from("pkg"), modules,
        devtools_registry: Default::default(), parse_teaching: Vec::new(),
        used_core: HashSet::new(), ffi_callback_fns: HashSet::new(), cffi: CFfi::default(),
        comptime_inputs: Vec::new(), name_ledger, layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core, web_partitions: HashMap::new(),
        web_partition_enforced: false, web_partition_report: None, dep_roots: HashMap::new(),
        package_guarantees: Default::default(), program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(), build_facts: Default::default(), edition: "2027".into(),
    };
    check_bundle(&mut bundle, CompileMode::Eval).into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what)).collect()
}

#[test]
fn absent_exit_guard_refines_byte_and_ok_lifts_into_optional_success_slot() {
    let source = r#"
fn read_byte(value: U8?) -> U8? Never! {
    byte :: value
    if byte == None -> return Ok(None)
    next_byte :: byte + U8{1}
    if byte == (Val(U8{10})) -> return Ok(next_byte)
    Ok(byte)
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn optional_success_lift_does_not_accept_a_different_payload_type() {
    let source = "fn wrong() -> U8? Never! { Ok(true) }\nfn run() {}\n";
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.iter().any(|error| error.starts_with("E0108:")), "{errors:?}");
}

#[test]
fn imported_module_type_resolves_associated_function() {
    let errors = check_sources(&[
        ("pkg/engine.jet", "engine", "struct StreamScanner { value: Int }\nimpl StreamScanner { fn new(value: Int) -> StreamScanner Never! { StreamScanner{value: value} } }\n", &[]),
        ("pkg/main.jet", "main", "use engine as scan\nfn value() -> Int Never! { scanner :: scan.StreamScanner.new(7)\n scanner.value }\nfn run() {}\n", &[("scan", 0)]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn sealed_core_module_type_resolves_associated_function() {
    let errors = check_sources(&[
        ("<corelib>/Core/Encoding/XmlEngine.jet", "__core_encoding_xml_engine", "struct StreamScanner { value: Int }\nimpl StreamScanner { fn new(value: Int) -> StreamScanner Never! { StreamScanner{value: value} } }\n", &[]),
        ("<corelib>/Core/Encoding/Xml.jet", "core_xml", "use __core_encoding_xml_engine as engine\nfn value() -> Int Never! { scanner :: engine.StreamScanner.new(7)\n scanner.value }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn imported_associated_function_keeps_method_visibility() {
    let errors = check_sources(&[
        ("pkg/engine.jet", "engine", "struct StreamScanner { value: Int }\nimpl StreamScanner { fn _new(value: Int) -> StreamScanner Never! { StreamScanner{value: value} } }\n", &[]),
        ("pkg/main.jet", "main", "use engine as scan\nfn value() -> Int Never! { scanner :: scan.StreamScanner._new(7)\n scanner.value }\nfn run() {}\n", &[("scan", 0)]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0605:")), "{errors:?}");
}

#[test]
fn list_static_owner_arguments_survive_fallible_constructors() {
    let source = include_str!("../../../Compiler/JetSema/Tests/CollectionStaticFixtures.jet");
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn list_static_omitted_owner_uses_contextual_success_type() {
    let source = r#"
fn strings() -> [String] Never! {
    List.try_with_capacity(2) ?? panic("allocation failed")
}
fn bytes() -> [U8] Never! {
    List.try_new() ?? panic("allocation failed")
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn list_static_explicit_owner_is_not_overridden_by_context() {
    let source = r#"
fn wrong() -> [String] Never! {
    List<U8>.try_new() ?? panic("allocation failed")
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(
        errors.iter().any(|error| error.starts_with("E0113:") || error.starts_with("E0108:")),
        "{errors:?}",
    );
}
