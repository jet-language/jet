use jet_sema::AST::{CFfi, Item, LoadedModule, ProgramBundle, Type};
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn check(src: &str) -> (ProgramBundle, Vec<Diagnostic>) {
    let (tokens, lex) = Lexer::lex(src);
    assert!(lex.is_empty(), "lexer diagnostics: {lex:?}");
    let mut program = Parser::parse(&tokens).expect("source parses");
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from("."),
        modules: vec![LoadedModule {
            path: PathBuf::from("generic_module_body.jet"),
            display: "generic_module_body.jet".into(),
            alias: "main".into(),
            imports: std::mem::take(&mut program.imports),
            items: std::mem::take(&mut program.items),
            source: src.into(),
            web_target_ceiling: program.web_target_ceiling,
            pub_file: program.pub_file,
            no_prelude: program.no_prelude,
            html_path: program.html_path,
            no_alloc_policy: program.no_alloc_policy,
        }],
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        cffi: CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: HashMap::new(),
        layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: HashMap::new(),
        active_os: Syntax::OsTarget::host(),
    };
    let diagnostics = check_bundle(&mut bundle, CompileMode::Eval);
    (bundle, diagnostics)
}

fn error_codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter(|d| d.severity == Severity::Error).map(|d| d.code.as_ref()).collect()
}

#[test]
fn trait_impl_and_error_conversion_are_specialized_as_one_local_identity_graph() {
    let src = r#"
module Laws<T> {
    trait Reveal { type Output; fn reveal(self) -> T }
    struct Wrapped { value: T }
    impl Wrapped.Reveal { type Output = T; fn reveal(self) -> T { return self.value } }
    enum SourceErr { Bad(T) }
    enum TargetErr { Wrapped(SourceErr) }
    impl SourceErr -> TargetErr { return TargetErr.Wrapped(self) }
}
module IntLaws = Laws<Int>
fn run() {}
"#;
    let (bundle, diagnostics) = check(src);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    assert!(items.iter().any(|item| matches!(item, Item::Trait(t) if t.name == "IntLaws__Reveal" && t.methods[0].return_type == Some(Type::Int))));
    assert!(items.iter().any(|item| matches!(item, Item::Impl(i) if i.type_name == "IntLaws__Wrapped" && i.trait_name.as_deref() == Some("IntLaws__Reveal") && i.assoc_type_impls[0].2 == Type::Int)));
    assert!(items.iter().any(|item| matches!(item, Item::ErrorConv(ec) if ec.from_ty == "IntLaws__SourceErr" && ec.to_ty == "IntLaws__TargetErr")));
}

#[test]
fn duplicate_error_conversion_inside_instance_keeps_coherence_diagnostic() {
    let (_, diagnostics) = check(r#"
module Bad<T> {
    enum Source { Bad(T) }
    enum Target { Wrapped(Source) }
    impl Source -> Target { return Target.Wrapped(self) }
    impl Source -> Target { return Target.Wrapped(self) }
}
module Use = Bad<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E2405"]);
}

#[test]
fn generic_module_does_not_launder_error_conversion_orphans() {
    let (_, diagnostics) = check(r#"
module Bad<T> { impl Int -> String { return "number" } }
module Use = Bad<Int>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).contains(&"E2406"), "{diagnostics:#?}");
}
