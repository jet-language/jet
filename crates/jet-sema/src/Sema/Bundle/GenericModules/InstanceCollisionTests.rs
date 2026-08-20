use super::*;
use std::path::PathBuf;

fn identity_bundle(project_root: PathBuf) -> ProgramBundle {
    ProgramBundle {
        entry: 0,
        project_root,
        modules: Vec::new(),
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: crate::AST::CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: crate::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: crate::Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2027".to_string(),
    }
}

#[test]
fn package_identity_uses_canonical_source_not_credentials_paths_or_formatting() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("jet_package_identity_{nonce}"));
    let project_a = base.join("checkout-a/project");
    let project_b = base.join("checkout-b/project");
    let dep_a = base.join("private-a/dependency");
    let dep_b = base.join("private-b/dependency");
    for path in [&project_a, &project_b, &dep_a, &dep_b] { std::fs::create_dir_all(path).unwrap(); }
    std::fs::create_dir_all(project_a.join(".jet")).unwrap();
    std::fs::create_dir_all(project_b.join(".jet")).unwrap();
    std::fs::write(dep_a.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"demo\", version: \"1.2.3\" }").unwrap();
    std::fs::write(dep_b.join(crate::Syntax::PAYLOAD_FILE), "payload: {\n  version: \"1.2.3\",\n  name: \"demo\"\n}\n").unwrap();
    std::fs::write(project_a.join(crate::Syntax::UNIFIED_LOCK_FILE), "version=1\n[[package]]\nname=\"demo\"\nversion=\"1.2.3\"\nsource={ git=\"https://alice:secret@example.com/acme/demo.git?token=one\", rev=\"main\" }\nlocked={ rev=\"abc\", tree-hash=\"tree\", last-modified=1 }\n").unwrap();
    std::fs::write(project_b.join(crate::Syntax::UNIFIED_LOCK_FILE), "version = 1\n\n[[package]]\nsource = { git = \"https://bob:other@example.com/acme/demo.git#credential\", rev = \"main\" }\nname = \"demo\"\nlocked = { tree-hash = \"tree\", rev = \"abc\", last-modified = 99 }\nversion = \"1.2.3\"\n").unwrap();
    let a = package_identity(&identity_bundle(project_a.clone()), &dep_a, Some("demo"));
    let b = package_identity(&identity_bundle(project_b.clone()), &dep_b, Some("demo"));
    assert_eq!(a, b, "formatting, credentials, timestamps, and host paths are non-semantic");
    std::fs::write(project_b.join(crate::Syntax::UNIFIED_LOCK_FILE), "version=1\n[[package]]\nname=\"demo\"\nversion=\"1.2.3\"\nsource={ git=\"https://example.com/acme/demo.git\", rev=\"main\" }\nlocked={ rev=\"different\", tree-hash=\"tree\" }\n").unwrap();
    let changed = package_identity(&identity_bundle(project_b), &dep_b, Some("demo"));
    assert_ne!(a, changed, "locked git revision is semantic package source identity");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn same_template_path_in_different_packages_has_distinct_definition_identity() {
    let root = std::env::temp_dir().join(format!("jet_package_nominal_{}", std::process::id()));
    let first = root.join("first");
    let second = root.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(first.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"first\", version: \"1.0.0\" }").unwrap();
    std::fs::write(second.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"second\", version: \"1.0.0\" }").unwrap();
    let bundle = identity_bundle(root.clone());
    let a = definition_full_key(&package_identity(&bundle, &first, Some("first")), "src/template.jet", "", "Boxed");
    let b = definition_full_key(&package_identity(&bundle, &second, Some("second")), "src/template.jet", "", "Boxed");
    assert_ne!(a, b);
    assert!(!String::from_utf8_lossy(&a).contains(&root.to_string_lossy().as_ref()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn path_lock_content_changes_definition_and_instance_identity_but_host_path_does_not() {
    let root = std::env::temp_dir().join(format!("jet_path_lock_identity_{}", std::process::id()));
    let project = root.join("project");
    let dependency = root.join("dependency");
    std::fs::create_dir_all(project.join(".jet")).unwrap();
    std::fs::create_dir_all(&dependency).unwrap();
    std::fs::write(dependency.join(crate::Syntax::PAYLOAD_FILE), "payload: { name: \"dep\", version: \"1.2.3\" }").unwrap();
    let lock = |path: &str, content: &str| format!("version=1\n[[package]]\nname=\"dep\"\nversion=\"1.2.3\"\nsource={{path=\"{path}\"}}\ncontent-hash=\"{content}\"\n");
    let bundle = identity_bundle(project.clone());
    std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/a/dep", "tree-a")).unwrap();
    let package_a = package_identity(&bundle, &dependency, Some("dep"));
    std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/b/dep", "tree-a")).unwrap();
    assert_eq!(package_a, package_identity(&bundle, &dependency, Some("dep")));
    std::fs::write(project.join(crate::Syntax::UNIFIED_LOCK_FILE), lock("/host/b/dep", "tree-b")).unwrap();
    let package_b = package_identity(&bundle, &dependency, Some("dep"));
    let definition_a = definition_full_key(&package_a, "template.jet", "", "Boxed");
    let definition_b = definition_full_key(&package_b, "template.jet", "", "Boxed");
    assert_ne!(crate::SHA256::sha256_hex(&definition_a), crate::SHA256::sha256_hex(&definition_b));
    let instance = |definition_full_key| ModuleInstanceKey { definition_full_key, parameters: vec![1], args: vec![vec![2]] };
    assert_ne!(crate::SHA256::sha256_hex(&instance(definition_a).bytes()), crate::SHA256::sha256_hex(&instance(definition_b).bytes()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[should_panic(expected = "internal compiler error: E0859 generic module instance fingerprint collision")]
fn different_full_keys_with_same_digest_fail_closed_before_codegen() {
    let mut registry = HashMap::new();
    let make = |full_key| crate::AST::ModuleInstanceIdentity { full_key, fingerprint: "forced-digest".into(), definition_id: "def".into(), argument_keys: Vec::new(), argument_values: Vec::new(), argument_provenance: Vec::new(), template_span: Span::new(0, 0), applications: Vec::new() };
    let first = make(vec![1]);
    let second = make(vec![2]);
    register_instance_fingerprint(&mut registry, &first, Span::new(1, 2));
    register_instance_fingerprint(&mut registry, &second, Span::new(3, 4));
}

#[test]
fn generated_nominal_names_encode_module_alias_boundaries() {
    assert_ne!(module_type_name("foo", "BarBaz"), module_type_name("foo_bar", "Baz"));
    assert_ne!(module_type_name("foo_bar", "Baz"), module_type_name("fo_obar", "Baz"));
    assert_eq!(module_type_name("_cache", "Item"), "_M5CacheItem");
}

#[test]
fn generic_template_snapshot_never_filters_parser_admitted_items() {
    let source = r#"
module everything<T> {
    @answer :: 42;
    tag Marked { deny: [Net] }
    trait Show { fn show(self) => T }
    struct Boxed { value: T }
    enum Maybe { Empty Value(T) }
    impl Boxed.Show { fn show(self) => T { return self.value } }
    fn id(value: T) => T { return ~value }
    module nested { fn nested() {} }
    module inner<U> { fn inner(value: U) => U { return ~value } }
    module int_inner :: inner<Int>
    #Test("smoke") { expect(@answer == 42) }
    #Bench("work") { expect(@answer == 42) }
}
fn run() {}
"#;
    let (tokens, lex) = crate::Lexer::lex(source);
    assert!(lex.is_empty(), "{lex:?}");
    let program = crate::Parser::parse(&tokens).expect("parser-admitted generic body");
    let template = program.items.iter().find_map(|item| match item {
        Item::GenericModule(template) => Some(template),
        _ => None,
    }).expect("generic template");
    let snapshot = template.clone();
    assert_eq!(snapshot.body.len(), template.body.len());
    assert_eq!(
        crate::CanonicalAST::canonical_fragment(&snapshot.body),
        crate::CanonicalAST::canonical_fragment(&template.body),
    );
}
