use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use jet_sema::Diagnostics::Diagnostic;
use jet_sema::Sema::{check_bundle_with_effect_facts_for_build, CompileMode};
use jet_sema::{Lexer, Parser};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn ensure_tir_bridge() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        jet_codegen::Codegen::TIR::install_comptime_bridge();
    });
}

fn check(source: &str) -> Vec<Diagnostic> {
    ensure_tir_bridge();
    let (tokens, lex) = Lexer::lex(source);
    assert!(lex.is_empty(), "lexer diagnostics: {lex:?}");
    let mut program = Parser::parse(&tokens).expect("source parses");
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from("."),
        modules: vec![LoadedModule {
            path: PathBuf::from("comptime_import_liveness.jet"),
            display: "comptime_import_liveness.jet".into(),
            alias: "main".into(),
            imports: std::mem::take(&mut program.imports),
            items: std::mem::take(&mut program.items),
            script_body: std::mem::take(&mut program.script_body),
            source: source.into(),
            block_spans: std::mem::take(&mut program.block_spans),
            web_target_ceiling: program.web_target_ceiling,
            pub_file: program.pub_file,
            no_prelude: program.no_prelude,
            default_target: program.default_target,
            html_path: program.html_path,
            policy_declarations: program.policy_declarations.clone(),
            user_policy_declarations: program.user_policy_declarations.clone(),
            rule_facts: std::mem::take(&mut program.rule_facts),
        }],
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: jet_sema::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: jet_sema::Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: jet_sema::Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2028".to_string(),
    };
    check_bundle_with_effect_facts_for_build(&mut bundle, CompileMode::Run).0
}

fn lint_names(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "L0103")
        .filter_map(|diagnostic| diagnostic.what.split('`').nth(1))
        .collect()
}

#[test]
fn comptime_qualified_import_shadowed_by_lambda_parameter_stays_unused() {
    let diagnostics = check(
        "use core.compiler as compiler\n@shadowed :: ((compiler: String) -> compiler.len())(\"x\")\nfn run() {}\n",
    );
    assert_eq!(
        lint_names(&diagnostics),
        vec!["compiler"],
        "lambda parameter shadowing must not consume the import: {diagnostics:#?}"
    );
}

#[test]
fn failed_comptime_qualified_import_stays_unused() {
    let diagnostics = check(
        "use core.compiler as compiler\n@failed :: compiler.not_a_real_call()\nfn run() {}\n",
    );
    assert!(
        lint_names(&diagnostics).contains(&"compiler"),
        "failed qualified comptime resolution must not consume the import: {diagnostics:#?}"
    );
}
