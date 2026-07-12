use jet_sema::AST::{CFfi, Item, LoadedModule, ProgramBundle, Type};
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn check(src: &str) -> (ProgramBundle, Vec<Diagnostic>) {
    check_at(src, ".")
}

fn check_at(src: &str, root: &str) -> (ProgramBundle, Vec<Diagnostic>) {
    let (tokens, lex) = Lexer::lex(src);
    assert!(lex.is_empty(), "lexer diagnostics: {lex:?}");
    let mut program = Parser::parse(&tokens).expect("source parses");
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from(root),
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

fn only_instance_fingerprint(src: &str, root: &str) -> String {
    let (bundle, diagnostics) = check_at(src, root);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    bundle.modules[0].items.iter().find_map(|item| match item {
        Item::CodeModule(module) => module.instance_identity.as_ref().map(|identity| identity.fingerprint.clone()),
        _ => None,
    }).expect("generic instance fingerprint")
}

fn check_modules(sources: &[(&str, &str, &[(&str, usize)])]) -> (ProgramBundle, Vec<Diagnostic>) {
    let mut modules = Vec::new();
    let mut import_targets = HashMap::new();
    for (module_idx, (path, src, targets)) in sources.iter().enumerate() {
        let (tokens, lex) = Lexer::lex(src);
        assert!(lex.is_empty(), "lexer diagnostics: {lex:?}");
        let mut program = Parser::parse(&tokens).expect("source parses");
        for (alias, target) in *targets {
            let import = program.imports.iter().find(|import| import.import_alias() == *alias && !matches!(import.kind, jet_sema::AST::ImportKind::Unqualified { .. })).unwrap();
            import_targets.insert((module_idx, import.span), *target);
        }
        modules.push(LoadedModule {
            path: PathBuf::from(path), display: (*path).into(), alias: path.trim_end_matches(".jet").into(),
            imports: std::mem::take(&mut program.imports), items: std::mem::take(&mut program.items), source: (*src).into(),
            web_target_ceiling: program.web_target_ceiling, pub_file: program.pub_file, no_prelude: program.no_prelude,
            html_path: program.html_path, no_alloc_policy: program.no_alloc_policy,
        });
    }
    let mut bundle = ProgramBundle {
        entry: sources.len() - 1, project_root: PathBuf::from("pkg-a"), modules,
        parse_teaching: Vec::new(), used_core: HashSet::new(), cffi: CFfi::default(), comptime_inputs: Vec::new(),
        import_targets, layer_ceiling: None, inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(), web_partition_enforced: false, web_partition_report: None,
        dep_roots: HashMap::new(), active_os: Syntax::OsTarget::host(),
    };
    let diagnostics = check_bundle(&mut bundle, CompileMode::Eval);
    (bundle, diagnostics)
}

fn error_codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter(|d| d.severity == Severity::Error).map(|d| d.code.as_ref()).collect()
}

#[test]
fn equivalent_instances_are_interned_and_project_one_nominal_identity() {
    let src = r#"
module Boxed<T, size: Int> {
    struct Box { value: T }
    fn identity(value: Box) -> Box { return copy value }
}

module Other<T, size: Int> { struct Box { value: T } }
module First = Boxed<Int, 3>
module Equivalent = Boxed<Int, 3>
module Forward = Equivalent
module DifferentType = Boxed<String, 3>
module DifferentValue = Boxed<Int, 4>
module DifferentTemplate = Other<Int, 3>
fn accepts_first(value: First__Box) -> First__Box { return copy value }
fn accepts_projection(value: Equivalent__Box) -> First__Box { return copy value }
fn accepts_chain(value: Forward__Box) -> First__Box { return copy value }
fn run() {}
"#;
    let (bundle, diagnostics) = check(src);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    let modules: Vec<&str> = items.iter().filter_map(|item| match item {
        Item::CodeModule(module) => Some(module.name.as_str()),
        _ => None,
    }).collect();
    assert_eq!(modules.iter().filter(|name| **name == "First").count(), 1);
    assert!(!modules.contains(&"Equivalent"));
    assert!(!modules.contains(&"Forward"));
    assert!(modules.contains(&"DifferentType"));
    assert!(modules.contains(&"DifferentValue"));
    assert!(modules.contains(&"DifferentTemplate"));
    assert_eq!(items.iter().filter(|item| matches!(item, Item::Struct(def) if def.name == "First__Box")).count(), 1);
    for name in ["accepts_projection", "accepts_chain"] {
        let Item::Func(func) = items.iter().find(|item| matches!(item, Item::Func(func) if func.name == name)).unwrap() else { unreachable!() };
        assert_eq!(func.params[0].ty, Type::Named("First__Box".into()));
        assert_eq!(func.return_type, Some(Type::Named("First__Box".into())));
    }
}

#[test]
fn imported_template_is_interned_once_across_consumers() {
    let template = r#"
pub module Boxed<T, size: Int> { pub struct Box { value: T } }
pub module Other<T, size: Int> { pub struct Box { value: T } }
"#;
    let first = r#"
use "./templates" as templates
use templates.{Boxed}
pub module First = Boxed<Int, 3>
fn run() {}
"#;
    let second = r#"
use "./templates" as templates
use templates.{Boxed, Other}
module Second = Boxed<Int, 3>
module DifferentArg = Boxed<Int, 4>
module DifferentTemplate = Other<Int, 3>
fn accepts_projection(value: Second__Box) -> First__Box { return copy value }
fn run() {}
"#;
    let (bundle, diagnostics) = check_modules(&[
        ("templates.jet", template, &[]),
        ("first.jet", first, &[("templates", 0)]),
        ("second.jet", second, &[("templates", 0)]),
    ]);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    assert_eq!(bundle.modules.iter().flat_map(|module| &module.items).filter(|item| matches!(item, Item::Struct(def) if def.name == "First__Box")).count(), 1);
    assert!(!bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "Second")));
    assert!(bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "DifferentArg")));
    assert!(bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "DifferentTemplate")));
}

#[test]
fn instance_fingerprint_ignores_spans_but_invalidates_semantic_inputs() {
    let base = "module Boxed<T, n: Int> { fn value() -> Int { return n } }\nmodule Use = Boxed<Int, 3>\nfn run() {}";
    let shifted = "\n\nmodule Boxed<T, n: Int> {   fn value() -> Int { return n } }\nmodule Renamed = Boxed<Int, 3>\nfn run() {}";
    let body = "module Boxed<T, n: Int> { fn value() -> Int { return n + 1 } }\nmodule Use = Boxed<Int, 3>\nfn run() {}";
    let arg = "module Boxed<T, n: Int> { fn value() -> Int { return n } }\nmodule Use = Boxed<Int, 4>\nfn run() {}";
    let fp = only_instance_fingerprint(base, "pkg-a");
    assert_eq!(fp, only_instance_fingerprint(shifted, "pkg-a"));
    assert_ne!(fp, only_instance_fingerprint(body, "pkg-a"));
    assert_ne!(fp, only_instance_fingerprint(arg, "pkg-a"));
    assert_ne!(fp, only_instance_fingerprint(base, "pkg-b"));
}

#[test]
fn trait_impl_and_error_conversion_are_specialized_as_one_local_identity_graph() {
    let src = r#"
module Laws<T> {
    tag Audited;
    fn audited(value: #Audited T) -> #Audited T { return copy value }
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
    assert!(items.iter().any(|item| matches!(item, Item::Tag(t) if t.name == "IntLaws__Audited")));
    let Item::CodeModule(instance) = items.iter().find(|item| matches!(item, Item::CodeModule(m) if m.name == "IntLaws")).unwrap() else { unreachable!() };
    let tagged = instance.body.as_ref().unwrap().iter().find_map(|item| match item { Item::Func(f) if f.name == "audited" => Some(&f.params[0].ty), _ => None }).unwrap();
    assert!(matches!(tagged, Type::Tagged { marker, inner } if marker == "IntLaws__Audited" && **inner == Type::Int), "{tagged:?}");
    assert!(items.iter().any(|item| matches!(item, Item::Impl(i) if i.type_name == "IntLaws__Wrapped" && i.trait_name.as_deref() == Some("IntLaws__Reveal") && i.assoc_type_impls[0].2 == Type::Int)));
    assert!(items.iter().any(|item| matches!(item, Item::ErrorConv(ec) if ec.from_ty == "IntLaws__SourceErr" && ec.to_ty == "IntLaws__TargetErr")));
}

#[test]
fn tag_method_inside_instance_keeps_e0732() {
    let (_, diagnostics) = check(r#"
module Bad<T> { tag Marker { fn forbidden(self) -> T; } }
module Use = Bad<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E0732"]);
}

#[test]
fn deriving_instance_tag_keeps_e0731() {
    let (_, diagnostics) = check(r#"
module Bad<T> {
    tag Marker;
    struct Value { item: T; derive Marker }
}
module Use = Bad<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E0731"]);
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

#[test]
fn nested_generic_alias_closes_over_outer_type_and_value_arguments() {
    let (bundle, diagnostics) = check(r#"
module Outer<T, count: Int> {
    module Inner<U> { pub fn keep(value: T, other: U) -> T { return value } }
    module Fixed = Inner<Int>
    module Forward = Fixed
}
module TextOuter = Outer<String, 3>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let Item::CodeModule(outer) = bundle.modules[0].items.iter()
        .find(|item| matches!(item, Item::CodeModule(m) if m.name == "TextOuter")).unwrap() else { unreachable!() };
    let inner = outer.body.as_ref().unwrap().iter().find_map(|item| match item {
        Item::CodeModule(module) if module.name == "TextOuter__Fixed" => Some(module), _ => None,
    }).expect("nested alias expanded");
    let Item::Func(keep) = &inner.body.as_ref().unwrap()[0] else { panic!("nested function") };
    assert_eq!(keep.params[0].ty, Type::String);
    assert_eq!(keep.params[1].ty, Type::Int);
    assert_eq!(keep.return_type, Some(Type::String));
    assert!(outer.body.as_ref().unwrap().iter().any(|item| matches!(item,
        Item::CodeModule(module) if module.name == "TextOuter__Forward")));
}

#[test]
fn tests_and_benches_are_specialized_once_per_instance_with_unique_names() {
    let (bundle, diagnostics) = check(r#"
module Checks<T, count: Int> {
    #Test fn identity(value: T) { expect(count == count) }
    #Bench("work") { expect(count == count) }
}
module IntChecks = Checks<Int, 2>
module TextChecks = Checks<String, 4>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    let tests: Vec<_> = items.iter().filter_map(|item| match item { Item::Test(t) => Some(t), _ => None }).collect();
    let benches: Vec<_> = items.iter().filter_map(|item| match item { Item::Bench(b) => Some(b), _ => None }).collect();
    assert_eq!(tests.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["IntChecks__identity", "TextChecks__identity"]);
    assert_eq!(tests[0].params[0].ty, Type::Int);
    assert_eq!(tests[1].params[0].ty, Type::String);
    assert_eq!(benches.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["IntChecks__work", "TextChecks__work"]);
    assert!(!format!("{:?}{:?}", tests[0].body, benches[0].body).contains("count"));
}
