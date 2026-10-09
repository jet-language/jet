//! Loop ownership follows executable back edges, and owned tables infer once.
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::{Lexer, Parser, Syntax};
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn errors(src: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = Lexer::lex(src);
    assert!(lex_diags.is_empty(), "{lex_diags:?}");
    let mut prog = Parser::parse(&toks).expect("ownership fixture parses");
    let mut bundle = ProgramBundle {
        entry: 0,
        project_root: PathBuf::from("."),
        modules: vec![LoadedModule {
            path: PathBuf::from("ownership_flow.jet"),
            display: "ownership_flow.jet".to_string(),
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
        dep_roots: HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2027".to_string(),
    };
    check_bundle(&mut bundle, CompileMode::Eval).into_iter()
        .filter(|d| d.severity == Severity::Error).collect()
}

#[test]
fn loop_exit_reset_and_owned_payloads_are_valid() {
    let found = errors(include_str!("../../../tests/run/ownership/loop_exit_reset_owned_match.jet"));
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn a_move_on_a_real_backedge_is_still_rejected() {
    for exit in ["", "next"] {
        let src = format!(r#"
fn take(value: ^[Int]) -[]> Int {{ value.len() }}
fn run() {{
    current := [1]
    loop i in 0..<3 {{
        print(take(^current))
        {exit}
    }}
}}
"#);
        let found = errors(&src);
        assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    }
}

#[test]
fn owned_match_does_not_restore_the_consumed_owner() {
    let found = errors(r#"
enum Tree { Object([Int]) Text(String) }
fn run() {
    owned := Tree.Object([1])
    if ^owned == {
        .Object(entries) -> print(entries.len())
        .Text(text) -> print(text.count_bytes())
    }
    print(owned)
}
"#);
    assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    assert!(!found.iter().any(|d| d.code == "E0107"), "payloads must bind: {found:?}");
}

#[test]
fn an_explicit_constructor_payload_stays_moved() {
    let found = errors(r#"
use core.encoding.[Dyn]
fn run() {
    text := "owned"
    value :: Dyn.Text(^text)
    print(value)
    print(text)
}
"#);
    assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    assert!(!found.iter().any(|d| d.code == "E0203"), "{found:?}");
}

#[test]
fn a_completed_move_before_a_loop_is_not_a_backedge_move() {
    // stream.jet wire_float: an unsafe block takes shown before scanning bytes.
    let found = errors(r#"
fn take(value: ^String) -[]> Int { value.count_bytes() }
fn run() {
    shown :: "1.25"
    count := 0
    #Unsafe("exercise the source block boundary") { count = take(^shown) }
    loop i in 0..<count { print(i) }
}
"#);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn taking_a_parameter_before_nested_loops_is_not_repeated() {
    // cbor_stream.jet close_frame: input moves at function entry.
    let found = errors(r#"
fn consume(input: ^[[Int]]) -[]> Int {
    frame := ^input
    total := 0
    loop item in frame {
        loop number in item { total += number }
    }
    total
}
fn run() { print(consume([[1], [2]])) }
"#);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_owned_value_table_binds_payloads_and_takes_its_subject_once() {
    // sys.jet config_insert: a table is a value tail, not statement returns.
    let found = errors(r#"
enum Tree { Object([Int]) Text(String) }
fn rebuild(tree: ^Tree) -[]> Tree {
    owned := ^tree
    if ^owned == {
        .Object(pairs) -> {
            out := ^pairs
            loop index in 0..<out.len() { print(index) }
            Tree.Object(^out)
        }
        .Text(text) -> Tree.Text(^text)
    }
}
fn run() { print(rebuild(Tree.Text("owned"))) }
"#);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_owned_value_table_can_take_a_collection_payload_in_its_loop_header() {
    // sys.jet config_tree: the collection move belongs to the header, not body.
    let found = errors(r#"
enum Tree { Object([[Int]]) Text(String) }
fn count(tree: ^Tree) -[]> Int {
    owned := ^tree
    if ^owned == {
        .Text(text) -> text.count_bytes()
        .Object(pairs) -> {
            total := 0
            loop pair in ^pairs { total += pair.len() }
            total
        }
    }
}
fn run() { print(count(Tree.Object([[1], [2]]))) }
"#);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn range_and_collection_backedges_still_reject_exact_outer_moves() {
    for header in ["i in 0..<3", "i in [1, 2, 3]"] {
        let source = format!(r#"
fn take(value: ^[Int]) -[]> Int {{ value.len() }}
fn run() {{
    current := [1]
    loop {header} {{ print(take(^current)) }}
}}
"#);
        let found = errors(&source);
        assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    }
}
