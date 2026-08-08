use jet_sema::AST::{CFfi, Item, LoadedModule, ProgramBundle, TagMarker, Type};
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn ensure_tir_bridge() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        jet_codegen::Codegen::TIR::install_comptime_bridge();
    });
}

fn check(src: &str) -> (ProgramBundle, Vec<Diagnostic>) {
    check_at(src, ".")
}

fn check_at(src: &str, root: &str) -> (ProgramBundle, Vec<Diagnostic>) {
    ensure_tir_bridge();
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
            block_spans: std::mem::take(&mut program.block_spans),
            web_target_ceiling: program.web_target_ceiling,
            pub_file: program.pub_file,
            no_prelude: program.no_prelude,
            html_path: program.html_path,
            no_alloc_policy: program.no_alloc_policy,
            policy_declarations: program.policy_declarations.clone(),
            rule_facts: std::mem::take(&mut program.rule_facts),
        }],
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: HashMap::new(),
        layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: HashMap::new(),
        active_os: Syntax::OSTarget::host(),
        edition: "2027".to_string(),
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
    ensure_tir_bridge();
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
            block_spans: std::mem::take(&mut program.block_spans),
            web_target_ceiling: program.web_target_ceiling, pub_file: program.pub_file, no_prelude: program.no_prelude,
            html_path: program.html_path, no_alloc_policy: program.no_alloc_policy,
            policy_declarations: program.policy_declarations.clone(),
            rule_facts: std::mem::take(&mut program.rule_facts),
        });
    }
    let mut bundle = ProgramBundle {
        entry: sources.len() - 1, project_root: PathBuf::from("pkg-a"), modules,
        parse_teaching: Vec::new(), used_core: HashSet::new(), ffi_callback_fns: HashSet::new(), cffi: CFfi::default(), comptime_inputs: Vec::new(),
        import_targets, layer_ceiling: None, inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(), web_partition_enforced: false, web_partition_report: None,
        dep_roots: HashMap::new(),
        active_os: Syntax::OSTarget::host(),
        edition: "2027".to_string(),
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
module boxed<T, size: Int> {
    struct Box { value: T }
    fn identity(value: Box) => Box { return ~value }
}
module other<T, size: Int> { struct Box { value: T } }
module first = boxed<Int, 3>
module equivalent = boxed<Int, 3>
module forward = equivalent
module different_type = boxed<String, 3>
module different_value = boxed<Int, 4>
module different_template = other<Int, 3>
fn accepts_first(value: M5FirstBox) => M5FirstBox { return ~value }
fn accepts_projection(value: M10EquivalentBox) => M5FirstBox { return ~value }
fn accepts_chain(value: M7ForwardBox) => M5FirstBox { return ~value }
fn run() {}
"#;
    let (bundle, diagnostics) = check(src);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    let modules: Vec<&str> = items.iter().filter_map(|item| match item {
        Item::CodeModule(module) => Some(module.name.as_str()),
        _ => None,
    }).collect();
    assert_eq!(modules.iter().filter(|name| **name == "first").count(), 1);
    assert!(!modules.contains(&"equivalent"));
    assert!(!modules.contains(&"forward"));
    assert!(modules.contains(&"different_type"));
    assert!(modules.contains(&"different_value"));
    assert!(modules.contains(&"different_template"));
    assert_eq!(items.iter().filter(|item| matches!(item, Item::Struct(def) if def.name == "M5FirstBox")).count(), 1);
    for name in ["accepts_projection", "accepts_chain"] {
        let Item::Func(func) = items.iter().find(|item| matches!(item, Item::Func(func) if func.name == name)).unwrap() else { unreachable!() };
        assert_eq!(func.params[0].ty, Type::Named("M5FirstBox".into()));
        assert_eq!(func.return_type, Some(Type::Named("M5FirstBox".into())));
    }
}

#[test]
fn soft_public_aliases_keep_distinct_casing_safe_type_names() {
    let (bundle, diagnostics) = check(r#"
module holder<T> { struct Item { value: T } }
module cache = holder<Int>
module _cache = holder<String>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let names: Vec<_> = bundle.modules[0].items.iter().filter_map(|item| match item {
        Item::Struct(def) => Some(def.name.as_str()),
        _ => None,
    }).collect();
    assert!(names.contains(&"M5CacheItem"), "{names:?}");
    assert!(names.contains(&"_M5CacheItem"), "{names:?}");
}

#[test]
fn imported_template_is_interned_once_across_consumers() {
    let template = r#"
pub module boxed<T, size: Int> { pub struct Box { value: T } }
pub module other<T, size: Int> { pub struct Box { value: T } }
"#;
    let first = r#"
use "./templates" as templates
use templates.{boxed}
pub module first = boxed<Int, 3>
fn run() {}
"#;
    let second = r#"
use "./templates" as templates
use templates.{boxed, other}
module second = boxed<Int, 3>
module different_arg = boxed<Int, 4>
module different_template = other<Int, 3>
fn accepts_projection(value: M6SecondBox) => M5FirstBox { return ~value }
fn run() {}
"#;
    let (bundle, diagnostics) = check_modules(&[
        ("templates.jet", template, &[]),
        ("first.jet", first, &[("templates", 0)]),
        ("second.jet", second, &[("templates", 0)]),
    ]);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    assert_eq!(bundle.modules.iter().flat_map(|module| &module.items).filter(|item| matches!(item, Item::Struct(def) if def.name == "M5FirstBox")).count(), 1);
    assert!(!bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "second")));
    assert!(bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "different_arg")));
    assert!(bundle.modules[2].items.iter().any(|item| matches!(item, Item::CodeModule(module) if module.name == "different_template")));
}

#[test]
fn instance_fingerprint_is_nominal_and_ignores_body_shape() {
    let base = "module boxed<T, n: Int> { fn value() => Int { return n } }\nmodule instance = boxed<Int, 3>\nfn run() {}";
    let shifted = "\n\nmodule boxed<T, n: Int> {   fn value() => Int { return n } }\nmodule renamed = boxed<Int, 3>\nfn run() {}";
    let body = "module boxed<T, n: Int> { fn value() => Int { return n + 1 } }\nmodule instance = boxed<Int, 3>\nfn run() {}";
    let arg = "module boxed<T, n: Int> { fn value() => Int { return n } }\nmodule instance = boxed<Int, 4>\nfn run() {}";
    let fp = only_instance_fingerprint(base, "pkg-a");
    assert_eq!(fp, only_instance_fingerprint(shifted, "pkg-a"));
    assert_eq!(fp, only_instance_fingerprint(body, "pkg-a"));
    assert_ne!(fp, only_instance_fingerprint(arg, "pkg-a"));
    assert_eq!(fp, only_instance_fingerprint(base, "pkg-b"), "manifest-less host paths are non-semantic");

    let roots = std::env::temp_dir().join(format!("jet_genmod_nominal_packages_{}", std::process::id()));
    let package_a = roots.join("checkout-a");
    let package_b = roots.join("checkout-b");
    std::fs::create_dir_all(&package_a).unwrap();
    std::fs::create_dir_all(&package_b).unwrap();
    std::fs::write(package_a.join("pkg.jet"), "payload: { name: \"package-a\", version: \"1.0.0\" }").unwrap();
    std::fs::write(package_b.join("pkg.jet"), "payload: { name: \"package-b\", version: \"1.0.0\" }").unwrap();
    assert_ne!(
        only_instance_fingerprint(base, package_a.to_str().unwrap()),
        only_instance_fingerprint(base, package_b.to_str().unwrap()),
        "canonical package identity remains nominal",
    );
    let _ = std::fs::remove_dir_all(roots);
}

#[test]
fn instance_definition_identity_tracks_manifest_semver_not_formatting_or_workspace_lock_noise() {
    let base = std::env::temp_dir().join(format!("jet_genmod_identity_{}", std::process::id()));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(a.join(".jet")).unwrap();
    std::fs::create_dir_all(b.join(".jet")).unwrap();
    std::fs::write(a.join("pkg.jet"), "payload: {\n name: \"demo\",\n version: \"1.0.0\"\n}").unwrap();
    std::fs::write(b.join("pkg.jet"), "payload: {\n name: \"demo\",\n version: \"2.0.0\"\n}").unwrap();
    std::fs::write(a.join(".jet/lock"), "source = a").unwrap();
    std::fs::write(b.join(".jet/lock"), "source = b").unwrap();
    let src = "module boxed<T> { fn value(v: T) => T { return v } }\nmodule instance = boxed<Int>\nfn run() {}";
    assert_ne!(only_instance_fingerprint(src, a.to_str().unwrap()), only_instance_fingerprint(src, b.to_str().unwrap()));
    std::fs::write(b.join("pkg.jet"), "payload: { name: \"demo\", version: \"1.0.0\" }").unwrap();
    assert_eq!(only_instance_fingerprint(src, a.to_str().unwrap()), only_instance_fingerprint(src, b.to_str().unwrap()));
    std::fs::write(b.join(".jet/lock"), "source = a\n").unwrap();
    assert_eq!(only_instance_fingerprint(src, a.to_str().unwrap()), only_instance_fingerprint(src, b.to_str().unwrap()));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn trait_impl_and_error_conversion_are_specialized_as_one_local_identity_graph() {
    let src = r#"
module laws<T> {
    tag Audited { deny: [Net] }
    fn audited(value: #Audited T) => #Audited T { return ~value }
    trait Reveal { type Output; fn reveal(self) => T }
    struct Wrapped { value: T }
    impl Wrapped.Reveal { type Output = T; fn reveal(self) => T { return self.value } }
    enum SourceErr { Bad(T) }
    enum TargetErr { Wrapped(SourceErr) }
    impl SourceErr => TargetErr { return TargetErr.Wrapped(self) }
}
module int_laws = laws<Int>
fn run() {}
"#;
    let (bundle, diagnostics) = check(src);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    assert!(items.iter().any(|item| matches!(item, Item::Trait(t) if t.name == "M3Int4LawsReveal" && t.methods[0].return_type == Some(Type::Int))));
    assert!(items.iter().any(|item| matches!(item, Item::Tag(t) if t.name == "M3Int4LawsAudited")));
    let Item::CodeModule(instance) = items.iter().find(|item| matches!(item, Item::CodeModule(m) if m.name == "int_laws")).unwrap() else { unreachable!() };
    let tagged = instance.body.as_ref().unwrap().iter().find_map(|item| match item { Item::Func(f) if f.name == "audited" => Some(&f.params[0].ty), _ => None }).unwrap();
    assert!(matches!(tagged, Type::Tagged { marker, inner } if matches!(marker, TagMarker::User(name) if name == "M3Int4LawsAudited") && **inner == Type::Int), "{tagged:?}");
    assert!(items.iter().any(|item| matches!(item, Item::Impl(i) if i.type_name == "M3Int4LawsWrapped" && i.trait_name.as_deref() == Some("M3Int4LawsReveal") && i.assoc_type_impls[0].2 == Type::Int)));
    assert!(items.iter().any(|item| matches!(item, Item::ErrorConv(ec) if ec.from_ty == "M3Int4LawsSourceErr" && ec.to_ty == "M3Int4LawsTargetErr")));
}

#[test]
fn tag_method_inside_instance_keeps_e0732() {
    // The parser owns the tag-method rejection now; the diagnostic text is
    // unchanged. The template body still may not smuggle a method through a
    // generic instance.
    let (tokens, lex) = Lexer::lex(
        r#"
module bad<T> { tag Marker { fn forbidden(self) => T; } }
module instance = bad<Int>
fn run() {}
"#,
    );
    assert!(lex.is_empty(), "lexer diagnostics: {lex:?}");
    let err = Parser::parse(&tokens).expect_err("tag methods are rejected at parse time");
    assert!(
        err.iter().any(|d| d.code == "E0732"),
        "expected E0732 in parse diagnostics: {err:?}"
    );
}

#[test]
fn deriving_instance_tag_keeps_e0731() {
    let (_, diagnostics) = check(r#"
module bad<T> {
    tag Marker { deny: [Net] }
    struct Value { item: T; derive Marker }
}
module instance = bad<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E0731"]);
}

#[test]
fn duplicate_error_conversion_inside_instance_keeps_coherence_diagnostic() {
    let (_, diagnostics) = check(r#"
module bad<T> {
    enum Source { Bad(T) }
    enum Target { Wrapped(Source) }
    impl Source => Target { return Target.Wrapped(self) }
    impl Source => Target { return Target.Wrapped(self) }
}
module instance = bad<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E2405"]);
}

#[test]
fn generic_module_does_not_launder_error_conversion_orphans() {
    let (_, diagnostics) = check(r#"
module bad<T> { impl Int => String { return "number" } }
module instance = bad<Int>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).contains(&"E2406"), "{diagnostics:#?}");
}

#[test]
fn nested_generic_alias_closes_over_outer_type_and_value_arguments() {
    let (bundle, diagnostics) = check(r#"
module outer<T, count: Int> {
    module inner<U> { pub fn keep(value: T, other: U) => T { return ~value } }
    module fixed = inner<Int>
    module forward = fixed
}
module text_outer = outer<String, 3>
module other_outer = outer<String, 4>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let inner = bundle.modules[0].items.iter().find_map(|item| match item {
        Item::CodeModule(module) if module.name == "text_outer_fixed" => Some(module), _ => None,
    }).expect("nested alias expanded into a checked module");
    let Item::Func(keep) = &inner.body.as_ref().unwrap()[0] else { panic!("nested function") };
    assert_eq!(keep.params[0].ty, Type::String);
    assert_eq!(keep.params[1].ty, Type::Int);
    assert_eq!(keep.return_type, Some(Type::String));
    assert!(!bundle.modules[0].items.iter().any(|item| matches!(item,
        Item::CodeModule(module) if module.name == "text_outer_forward")));
    let identity = inner.instance_identity.as_ref().expect("nested applicative identity");
    assert_eq!(
        identity.applications.iter().map(|application| application.name.as_str()).collect::<Vec<_>>(),
        vec!["text_outer_fixed", "text_outer_forward"],
    );
    let other = bundle.modules[0].items.iter().find_map(|item| match item {
        Item::CodeModule(module) if module.name == "other_outer_fixed" => Some(module), _ => None,
    }).expect("different enclosing values keep distinct nested identities");
    assert_ne!(
        identity.fingerprint,
        other.instance_identity.as_ref().unwrap().fingerprint,
    );
}

#[test]
fn ordinary_nested_module_recursively_expands_generic_aliases() {
    let (bundle, diagnostics) = check(r#"
module outer<T, count: Int> {
    module plain {
        module inner<U> { pub fn total(value: U) => Int { return count } }
        module closed = inner<T>
        pub fn result(value: T) => Int { return closed.total(value) }
    }
    pub fn result(value: T) => Int { return plain.result(value) }
}
module selected = outer<Int, 6>
fn run() { print(selected.result(1)) }
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    let plain = items.iter().find_map(|item| match item {
        Item::CodeModule(module) if module.name == "selected_plain" => Some(module),
        _ => None,
    }).expect("ordinary nested module remains a real checked module");
    assert!(!plain.body.as_ref().unwrap().iter().any(|item| matches!(
        item,
        Item::GenericModule(_) | Item::ModuleAlias(_)
    )));
    let closed = items.iter().find_map(|item| match item {
        Item::CodeModule(module) if module.name == "selected_plain_closed" => Some(module),
        _ => None,
    }).expect("contained generic alias expands to a real module");
    assert!(closed.instance_identity.is_some());
}

#[test]
fn ordinary_nested_module_rejects_unknown_generic_target() {
    let (_, diagnostics) = check(r#"
module outer<T> {
    module plain {
        module bad = missing<T>
    }
}
module selected = outer<Int>
fn run() {}
"#);
    assert_eq!(error_codes(&diagnostics), vec!["E0850"]);
}

#[test]
fn tests_and_benches_are_specialized_once_per_instance_with_unique_names() {
    let (bundle, diagnostics) = check(r#"
module checks<T, count: Int> {
    #Test fn identity(value: T) { expect(count == count) }
    #Bench("work") { expect(count == count) }
}
module int_checks = checks<Int, 2>
module text_checks = checks<String, 4>
fn run() {}
"#);
    assert!(error_codes(&diagnostics).is_empty(), "{diagnostics:#?}");
    let items = &bundle.modules[0].items;
    let tests: Vec<_> = items.iter().filter_map(|item| match item { Item::Test(t) => Some(t), _ => None }).collect();
    let benches: Vec<_> = items.iter().filter_map(|item| match item { Item::Bench(b) => Some(b), _ => None }).collect();
    assert_eq!(tests.iter().map(|t| t.name.as_deref()).collect::<Vec<_>>(), vec![Some("int_checks_identity"), Some("text_checks_identity")]);
    assert_eq!(tests[0].params[0].ty, Type::Int);
    assert_eq!(tests[1].params[0].ty, Type::String);
    assert_eq!(benches.iter().map(|b| b.name.as_deref()).collect::<Vec<_>>(), vec![Some("int_checks_work"), Some("text_checks_work")]);
    assert!(!format!("{:?}{:?}", tests[0].body, benches[0].body).contains("count"));
}
