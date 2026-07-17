//! D-MUSTUSE1 (c18iwxqx): `@MustUse` ignored-result diagnostics (E0419).

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
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: prog.policy_declarations.clone(),
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
        dep_roots: std::collections::HashMap::new(),
        active_os: Syntax::OsTarget::host(),
    };
    check_bundle(&mut bundle, CompileMode::Eval)
        .into_iter()
        .filter(|d: &Diagnostic| d.severity == Severity::Error)
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn must_use_fn_ignored_is_e0419() {
    let src = r#"
@MustUse fn ticket() -> Int {
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
@MustUse struct Receipt {
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
@MustUse fn ping() -> Int {
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
@MustUse enum Ticket {
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
@MustUse struct Token { id: Int }
@MustUse fn mint(id: Int) -> Token {
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
