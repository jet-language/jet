mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

fn fixture(path: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path)).unwrap()
}

#[test]
fn package_policy_cannot_authorize_unsafe() {
    let error = jet::Package::PackageFacts::parse(&fixture("policy_package_unsafe_allow/package.jet"), "test").unwrap_err();
    assert!(matches!(error, jet::Package::PackageParseError::BadMemoryPolicy { .. }));
}

#[test]
fn manifest_memory_denial_uses_the_effects_rights_tree() {
    let package = jet::Package::PackageFacts::parse(
        "name: \"memory\"\nversion: \"0.1.0\"\nauthority: .{ holds: { deny: [Mem.Alloc(above: 65536)] } }\n",
        "test",
    )
    .unwrap();
    assert_eq!(
        package.effects_deny,
        Some(vec!["Mem.Alloc(above: 65536)".to_string()])
    );
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
fn hosted_gc_policy_matches_all_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "memory/gc_cyclic",
        include_str!("../examples/features/expected/memory/gc_cyclic.out"),
    );
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
    let declaration = |scope, value, source: &str| jet::Policy::PolicyDeclaration { key: jet::Policy::PolicyKey::Unsafe, value, scope, span: jet::Diagnostics::Span::new(0, 0), target: None, source: source.to_string() };
    let result = jet::Policy::resolve(jet::Policy::PolicyKey::Unsafe, [
        declaration(jet::Policy::PolicyScope::Organization, jet::Policy::PolicyValue::Obligations, "/admin/policy.jet"),
        declaration(jet::Policy::PolicyScope::Package, jet::Policy::PolicyValue::Relaxed, "package.jet"),
    ]);
    assert!(matches!(result, Err(jet::Policy::PolicyError::Widening { .. })));
}

#[test]
fn organization_policy_document_is_exact_and_fails_closed() {
    let valid = jet::Package::parse_policy_document(
        "policy: .{ unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }\n",
    )
    .unwrap();
    assert_eq!(valid.len(), 3);
    assert_eq!(valid[0].value, jet::Policy::PolicyValue::Obligations);
    assert_eq!(valid[1].key, jet::Policy::PolicyKey::Impure);
    assert_eq!(valid[2].key, jet::Policy::PolicyKey::Nondeterministic);
    assert!(jet::Package::parse_policy_document("policy: .{ unsafe: .Obligations").is_err());
    assert!(jet::Package::parse_policy_document("policy: .{ unsafe: .Obligations }\npackage: .{}").is_err());
}

#[test]
fn package_policy_document_accepts_explicit_units() {
    let declarations = jet::Package::parse_policy_document(
        "policy: .{ explicit_units: true }\n",
    )
    .unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].key, jet::Policy::PolicyKey::ExplicitUnits);
    assert_eq!(declarations[0].value, jet::Policy::PolicyValue::Enabled);
}

#[test]
fn copies_policy_is_package_and_source_explicit_only() {
    let declarations = jet::Package::parse_policy_document(
        "policy: .{ copies: .Explicit }\n",
    )
    .unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].key, jet::Policy::PolicyKey::Copies);
    assert_eq!(declarations[0].value, jet::Policy::PolicyValue::Explicit);
    assert!(jet::Package::parse_policy_document("policy: .{ copies: true }\n").is_err());

    let source = "#Policy(copies: .Explicit)\nfn run() {}\n";
    let (tokens, lex) = jet::Lexer::lex(source);
    assert!(lex.is_empty());
    let program = jet::Parser::parse(&tokens).unwrap();
    assert!(program
        .policy_declarations
        .iter()
        .any(|declaration| declaration.key == jet::Policy::PolicyKey::Copies
            && declaration.value == jet::Policy::PolicyValue::Explicit));
}

#[test]
fn package_copies_policy_restores_string_view_refusal() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policy_copies_explicit/run.jet");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E2307"),
        "package copies policy must restore the refusal: {diagnostics:?}"
    );
}

#[test]
fn audited_gates_share_the_five_scope_tightening_ladder() {
    for key in jet::Policy::AUDITED_GATE_KEYS {
        let rule = jet::Policy::rule(*key);
        assert_eq!(rule.scopes.len(), 5, "{} must expose every policy scope", key.name());
        assert_eq!(rule.combine, jet::Policy::PolicyCombine::Tighten);
        assert_eq!(jet::Policy::PolicyKey::parse(key.name()), Some(*key));
        assert!(jet::Policy::parse_value(*key, ".Forbid").is_ok());
        assert_eq!(jet::Policy::GateSet::parse(&format!("{}=allow", key.name())), Ok(*key));
    }
}

#[test]
fn audited_gate_invocations_cannot_widen_an_organization_forbid() {
    for key in jet::Policy::AUDITED_GATE_KEYS {
        let declaration = jet::Policy::PolicyDeclaration {
            key: *key,
            value: jet::Policy::PolicyValue::Forbid,
            scope: jet::Policy::PolicyScope::Organization,
            span: jet::Diagnostics::Span::new(0, 0),
            target: None,
            source: "org-policy.jet".to_string(),
        };
        let result = jet::Policy::resolve_with_gates(*key, [declaration], &jet::Policy::GateSet::allow(*key));
        assert!(matches!(result, Err(jet::Policy::PolicyError::Widening { .. })), "{}", key.name());
    }
}

#[test]
fn audited_gate_invocations_are_allowed_under_tightening_modes() {
    for key in jet::Policy::AUDITED_GATE_KEYS {
        let declaration = jet::Policy::PolicyDeclaration {
            key: *key,
            value: jet::Policy::PolicyValue::Obligations,
            scope: jet::Policy::PolicyScope::Organization,
            span: jet::Diagnostics::Span::new(0, 0),
            target: None,
            source: "org-policy.jet".to_string(),
        };
        assert!(jet::Policy::resolve_with_gates(*key, [declaration], &jet::Policy::GateSet::allow(*key)).is_ok(), "{}", key.name());
    }
}

#[test]
fn audited_gate_ladder_example_allows_and_refuses_each_marker() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/effects/audited_gate_ladder.jet");
    for key in jet::Policy::AUDITED_GATE_KEYS {
        let mut allowed = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
        let mut all = jet::Policy::GateSet::default();
        for allowed_key in jet::Policy::AUDITED_GATE_KEYS {
            all.insert(*allowed_key);
        }
        let diagnostics = jet::Sema::check_bundle_gates(&mut allowed, jet::Sema::CompileMode::Run, all);
        assert!(diagnostics.is_empty(), "allowed {} gate: {diagnostics:#?}", key.name());

        let mut refused = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
        for module in &mut refused.modules {
            module.policy_declarations.push(jet::Policy::PolicyDeclaration {
                key: *key,
                value: jet::Policy::PolicyValue::Forbid,
                scope: jet::Policy::PolicyScope::Organization,
                span: jet::Diagnostics::Span::new(0, 0),
                target: None,
                source: "org-policy.jet".to_string(),
            });
        }
        let diagnostics = jet::Sema::check_bundle_gates(
            &mut refused,
            jet::Sema::CompileMode::Run,
            jet::Policy::GateSet::allow(*key),
        );
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3415"), "refused {} gate: {diagnostics:#?}", key.name());
    }
}
