//! D-FAIL-CONV2=A: core error family converts into default Err on demand.

use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Lexer;
use jet_sema::Parser;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::Syntax;
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn diags(src: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags;
    }
    let mut prog = match Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds,
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
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            default_target: prog.default_target,
            html_path: prog.html_path.clone(),
            policy_declarations: prog.policy_declarations.clone(),
            user_policy_declarations: prog.user_policy_declarations.clone(),
            rule_facts: std::mem::take(&mut prog.rule_facts),
        }],
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
    check_bundle(&mut bundle, CompileMode::Run)
        .into_iter()
        .filter(|d: &Diagnostic| d.severity == Severity::Error)
        .collect()
}

#[test]
fn core_json_error_converts_into_default_err() {
    let found = diags(
        r#"
use core.encoding.json as json
fn run() {
    data :: json.decode("{}")
    print(data)
}
"#,
    );
    assert!(
        !found.iter().any(|d| d.code == "E2402"),
        "core JSONError should convert into Err, got {:?}",
        found
            .iter()
            .map(|d| format!("{}: {}", d.code, d.what))
            .collect::<Vec<_>>()
    );
}
