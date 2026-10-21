//! D-MUSTUSE1 (c18iwxqx): `#MustUse` ignored-result diagnostics (E0419).

use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Lexer;
use jet_sema::Parser;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::Syntax;
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn err_codes(src: &str) -> Vec<String> {
    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags.into_iter().map(|d| d.code.to_string()).collect();
    }
    let mut prog = match Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds.into_iter().map(|d| d.code.to_string()).collect(),
    };
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from("."),
        modules: vec![LoadedModule {
            path: PathBuf::from("test.jet"),
            display: "test.jet".to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            script_body: std::mem::take(&mut prog.script_body),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            core_identifier_demand: Lexer::core_identifier_demand(&toks),
            web_target_ceiling: prog.web_target_ceiling,
            no_prelude: prog.no_prelude,
            default_target: prog.default_target,
            html_path: prog.html_path.clone(),
            policy_declarations: prog.policy_declarations.clone(),
            user_policy_declarations: prog.user_policy_declarations.clone(),
            rule_facts: std::mem::take(&mut prog.rule_facts),
        }],
        devtools_registry: jet_sema::AST::DevtoolsRegistry::default(),
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: jet_sema::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2027".to_string(),
    };
    check_bundle(&mut bundle, CompileMode::Eval)
        .into_iter()
        .filter(|d: &Diagnostic| d.severity == Severity::Error)
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn top_level_impl_method_keeps_owned_generic_scope() {
    let codes = err_codes(
        r#"
struct Counter { value: Int }

impl Counter {
    fn identity<T>(value: ^T) -> T {
        return value
    }
}

fn run() {}
"#,
    );
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn must_use_fn_ignored_is_e0419() {
    let src = r#"
#MustUse fn ticket() -> Int {
    return 1
}
fn run() {
    ticket()
}
"#;
    let codes = err_codes(src);
    assert!(
        codes.iter().any(|c| c == "E0419"),
        "expected E0419, got {:?}",
        codes
    );
}

#[test]
fn must_use_type_ignored_is_e0419() {
    let src = r#"
#MustUse struct Receipt {
    id: Int
}
fn issue() -> Receipt {
    return Receipt.{ id: 1 }
}
fn run() {
    issue()
}
"#;
    let codes = err_codes(src);
    assert!(
        codes.iter().any(|c| c == "E0419"),
        "expected E0419, got {:?}",
        codes
    );
}

#[test]
fn must_use_drop_suppresses_e0419() {
    let src = r#"
#MustUse fn ping() -> Int {
    return 1
}
fn run() {
    ping().drop("telemetry only")
}
"#;
    let codes = err_codes(src);
    assert!(
        !codes.iter().any(|c| c == "E0419"),
        "drop should silence E0419, got {:?}",
        codes
    );
}

#[test]
fn must_use_enum_ignored_is_e0419() {
    let src = r#"
#MustUse enum Ticket {
    Open
    Closed
}
fn issue() -> Ticket {
    return Ticket.Open
}
fn run() {
    issue()
}
"#;
    let codes = err_codes(src);
    assert!(
        codes.iter().any(|c| c == "E0419"),
        "expected E0419, got {:?}",
        codes
    );
}

#[test]
fn must_use_bound_value_ok() {
    let src = r#"
#MustUse struct Token { id: Int }
#MustUse fn mint(id: Int) -> Token {
    return Token.{ id: id }
}
fn run() {
    t := mint(1)
    print(t.id)
}
"#;
    let codes = err_codes(src);
    assert!(codes.is_empty(), "expected clean compile, got {:?}", codes);
}

#[test]
fn operator_hooks_resolve_by_rhs_in_binary_and_dot_calls() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/OperatorRhsFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn duplicate_operator_pair_is_coherence_error_not_method_name_collision() {
    let codes = err_codes(include_str!("../../../tests/ui/operator_duplicate_hook.jet"));
    assert_eq!(codes, ["E0908"], "unexpected errors: {codes:?}");
}

#[test]
fn scalar_conversion_heads_do_not_contextualize_arithmetic_operands() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/ScalarConversionFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn embedded_generic_methods_keep_owner_and_method_scopes() {
    let codes = err_codes(include_str!("../../../tests/ui/embedded_generic_method_scope.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn fallible_sibling_supplies_uncontextual_err_arm_carrier() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/fixtures/ErrArmJoinFixtures.fixture/ErrArmJoinFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn deferred_err_arm_keeps_branch_bindings_and_optional_flow() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/fixtures/ErrArmScopeFixtures.fixture/ErrArmScopeFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn deferred_err_arm_joins_actual_moves_and_checks_scope_end_after_payload() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/fixtures/ErrArmFlowFixtures.fixture/ErrArmFlowFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn user_err_function_keeps_ordinary_branch_call_semantics() {
    let codes = err_codes(include_str!("../../../Compiler/JetSema/Tests/ErrArmShadowFixtures.jet"));
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}
