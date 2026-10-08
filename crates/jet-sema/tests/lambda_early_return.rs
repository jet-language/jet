//! D-TAIL-RETURN1: early returns belong to their lambda's callable interface.

use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Lexer;
use jet_sema::Parser;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::Syntax;
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn errors(src: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = Lexer::lex(src);
    assert!(lex_diags.is_empty(), "{lex_diags:?}");
    let mut prog = Parser::parse(&toks).expect("lambda fixture parses");
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from("."),
        modules: vec![LoadedModule {
            path: PathBuf::from("lambda_early_return.jet"),
            display: "lambda_early_return.jet".to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            script_body: std::mem::take(&mut prog.script_body),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
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
        dep_roots: HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2027".to_string(),
    };
    check_bundle(&mut bundle, CompileMode::Eval)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

#[test]
fn sort_comparator_early_return_is_local_to_lambda() {
    let found = errors(include_str!("../../../tests/fixtures/lambda_early_return.jet"));
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}
