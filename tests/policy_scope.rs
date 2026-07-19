use jet::Policy::{self, PolicyKey, PolicyValue};

fn fixture(path: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path)).unwrap()
}

#[test]
fn package_policy_cannot_authorize_unsafe() {
    let error = jet::PackageManifest::parse(&fixture("policy_package_unsafe_allow/package.jet")).unwrap_err();
    assert!(matches!(error, jet::PackageManifest::ManifestError::BadMemoryPolicy { .. }));
}

#[test]
fn package_module_function_block_policy_has_one_explainable_chain() {
    let package = jet::PackageManifest::parse(&fixture("policy_scope_chain/package.jet")).unwrap();
    let source = fixture("policy_scope_chain/main.jet");
    let (tokens, lex) = jet::Lexer::lex(&source);
    assert!(lex.is_empty());
    let program = jet::Parser::parse(&tokens).unwrap();
    let mut declarations = package.memory_policy;
    declarations.extend(program.policy_declarations.iter().filter(|d| d.key == PolicyKey::ArenaBounded).cloned());
    let effective = Policy::resolve(PolicyKey::ArenaBounded, declarations).unwrap().unwrap();
    assert_eq!(effective.value, PolicyValue::Limit(8192));
    assert_eq!(effective.provenance.len(), 4);
    let explanation = Policy::explain(&effective);
    assert!(explanation.contains("package.jet"));
    assert!(explanation.contains("<source>"));
}

#[test]
fn package_to_module_widening_is_e0355_policy_error() {
    let package = jet::PackageManifest::parse(&fixture("policy_package_module_widen/package.jet")).unwrap();
    let source = fixture("policy_package_module_widen/main.jet");
    let (tokens, lex) = jet::Lexer::lex(&source);
    assert!(lex.is_empty());
    let program = jet::Parser::parse(&tokens).unwrap();
    let declarations = package.memory_policy.into_iter().chain(program.policy_declarations).collect::<Vec<_>>();
    assert!(matches!(Policy::resolve(PolicyKey::ArenaBounded, declarations), Err(Policy::PolicyError::Widening { .. })));
}

#[test]
fn package_gc_policy_applies_once_across_imported_modules() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policy_gc_multimodule/main.jet");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(!diagnostics.iter().any(|diagnostic| diagnostic.code == "E0355"));
    let function = bundle.modules[0]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "run" => Some(function),
            _ => None,
        })
        .unwrap();
    assert!(function.gc_scope);
    assert!(matches!(function.body.first(), Some(jet::AST::Stmt::Val(binding)) if binding.gc_promotion.is_some()));
}

#[test]
fn unsafe_obligations_are_checked_before_codegen() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unsafe_obligations/main.jet");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3107"));
    let function = bundle.modules[0].items.iter().find_map(|item| match item { jet::AST::Item::Func(function) if function.name == "run" => Some(function), _ => None }).unwrap();
    let jet::AST::Stmt::Unsafe { body, .. } = &function.body[1] else { panic!("unsafe gate") };
    assert!(!body.iter().any(|statement| matches!(statement, jet::AST::Stmt::Expr(jet::AST::Expr::Call(call)) if call.name == jet::Syntax::INTERNAL_UNSAFE_ASSERT)));
    let mut dev_bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let dev_codes = jet::Sema::check_bundle(&mut dev_bundle, jet::Sema::CompileMode::Check).into_iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();
    let run_codes = diagnostics.into_iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();
    assert_eq!(dev_codes, run_codes, "AOT and dev/check share the pre-TIR obligation pass");
}

#[test]
fn organization_obligations_floor_rejects_package_relaxation() {
    let declaration = |scope, value, source: &str| jet::Policy::PolicyDeclaration { key: PolicyKey::Unsafe, value, scope, span: jet::Diagnostics::Span::new(0, 0), target: None, source: source.to_string() };
    let result = Policy::resolve(PolicyKey::Unsafe, [
        declaration(jet::Policy::PolicyScope::Organization, PolicyValue::UnsafeObligations, "/admin/policy.jet"),
        declaration(jet::Policy::PolicyScope::Package, PolicyValue::UnsafeRelaxed, "package.jet"),
    ]);
    assert!(matches!(result, Err(Policy::PolicyError::Widening { .. })));
}

#[test]
fn organization_policy_document_is_exact_and_fails_closed() {
    let valid = jet::PackageManifest::parse_policy_document("policy: .{ unsafe: .Obligations }\n").unwrap();
    assert_eq!(valid.len(), 1);
    assert_eq!(valid[0].value, PolicyValue::UnsafeObligations);
    assert!(jet::PackageManifest::parse_policy_document("policy: .{ unsafe: .Obligations").is_err());
    assert!(jet::PackageManifest::parse_policy_document("policy: .{ unsafe: .Obligations }\npackage: .{}").is_err());
}

#[test]
fn package_policy_document_accepts_explicit_units() {
    let declarations = jet::PackageManifest::parse_policy_document(
        "policy: .{ explicit_units: true }\n",
    )
    .unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].key, PolicyKey::ExplicitUnits);
    assert_eq!(declarations[0].value, PolicyValue::Enabled);
}
