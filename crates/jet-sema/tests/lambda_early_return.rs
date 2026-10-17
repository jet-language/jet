//! D-TAIL-RETURN1: early returns belong to their lambda's callable interface.
//! The fixture carries lexer-owned Core demand and current public-default module facts.

use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Lexer;
use jet_sema::Parser;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::Syntax;
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn errors(src: &str) -> Vec<Diagnostic> {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(jet_codegen::Codegen::MIREval::install_mir_bridge);
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

#[test]
fn local_lambda_wins_over_same_named_file_function() {
    let found = errors(
        "fn text(key: String) -> String Never! { key.copy() }\n\
         fn object(key: Int) -> Int Never! { key }\n\
         fn valid() -> Bool Never! {\n\
             text :: (key: String) -[]> key == \"value\"\n\
             object :: (key: String) -[]> text(key)\n\
             object(\"value\")\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn non_callable_local_also_wins_over_file_function() {
    let found = errors(
        "fn text() -> Bool Never! { true }\n\
         fn valid() { text :: 1\n text() }\nfn run() {}\n",
    );
    assert!(found.iter().any(|d| d.code == "E0803"), "{found:?}");
}

#[test]
fn unsafe_tail_forwards_its_value_contract() {
    let found = errors(
        "fn value() -> U8 Never! {\n\
             #Unsafe(\"no raw operations in this reproduction\") {\n\
                 #Unsafe(\"nested tail also forwards\") { U8{7} }\n\
             }\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
    let missing = errors(
        "fn value() -> U8 Never! {\n\
             #Unsafe(\"empty block still has no value\") {}\n\
         }\nfn run() {}\n",
    );
    assert!(missing.iter().any(|d| d.code == "E0114"), "{missing:?}");
}

#[test]
fn direct_slice_argument_is_a_read_view() {
    let found = errors(
        "fn first(bytes: View<U8>) -> U8 Never! { bytes[0] }\n\
         fn value(bytes: [U8]) -> U8 Never! {\n\
             first(bytes[0..<bytes.len()])\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn char_crossings_and_infallible_list_reserve_do_not_invent_an_api() {
    for source in [
        "fn value(n: Int) -> Char Never! { Char{n} }\nfn run() {}\n",
        "fn value(c: Char) -> Int Never! { Int{c} }\nfn run() {}\n",
    ] {
        let found = errors(source);
        assert!(found.iter().any(|d| d.code == "E0108"), "{found:?}");
    }
    let found = errors(
        "fn value() Never! { bytes := [U8]{}\n &bytes.reserve(4) }\nfn run() {}\n",
    );
    assert!(found.iter().any(|d| d.code == "E0311"), "{found:?}");
}

#[test]
fn unannotated_local_predicates_keep_their_own_failure_contract() {
    let found = errors(
        "fn has(key: String) -> Bool { Err(\"global failure\") }\n\
         fn text(key: String) -> Bool { Err(\"global failure\") }\n\
         fn valid() -> Bool Never! {\n\
             has :: (key: String) -> !(key == \"\")\n\
             text :: (key: String) -> has(key)\n\
             text(\"value\")\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn braced_value_arm_next_is_loop_control() {
    let found = errors(
        "fn value() -> Int Never! {\n\
             loop { event :: 0\n\
                 node :: if event == { 0 -> { next }\n else -> 1 }\n\
                 return node\n\
             }\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn guard_payload_is_visible_inside_a_braced_arm() {
    let found = errors(
        "struct Name { prefix: String?, namespace_uri: String? }\n\
         struct NamespaceSet { fn insert(&self, key: String?) -> Bool Never! { true } }\n\
         fn value(name: Name) Never! {\n\
             visible := NamespaceSet{}\n\
             if name.prefix == .Val(prefix) { _ :: &visible.insert(Val(prefix)) } else if name.namespace_uri != None { _ :: &visible.insert(None) }\n\
         }\n\
         fn nested(name: String?) -> Int Never! {\n\
             if true -> {\n\
                 if name == .Val(prefix) { _ :: prefix.len() }\n\
                 1\n\
             } else -> 0\n\
         }\nfn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn shared_accepts_a_parenthesized_generic_optional_payload() {
    let found = errors(
        "fn value<T>(x: ^T) -> Shared<T?> { shared (T?{Val(^x)}) }\n\
         fn run() {}\n",
    );
    assert!(found.is_empty(), "unexpected errors: {found:?}");
}

#[test]
fn field_defaults_unwrap_total_constructor_calls_before_serde() {
    let src = r#"
struct E {
    n: Int
    fn empty() -> E Never! { E{n: 0} }
}
struct P { e: E{E.empty()} }
fn make() -> P Never! { P{} }
fn run() {}
"#;
    let found = errors(src);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn optional_value_equality_uses_lifting_not_a_duplicate_payload_binder() {
    let src = r#"
fn f(program: String) -> Bool Never! {
    v :: String?{None}
    answer :: v == program
    program.len() > 0 && answer
}
fn in_loop(program: String, rows: [String?]) -> Bool Never! {
    loop row in rows {
        if row == program -> return true
    }
    false
}
fn run() {}
"#;
    let found = errors(src);
    assert!(found.is_empty(), "{found:?}");
    let rejected = errors(r#"
fn f(program: String) -> Bool Never! {
    v :: String?{None}
    if v == Val(program) -> return true
    false
}
fn run() {}
"#);
    assert!(rejected.iter().any(|diagnostic| diagnostic.code == "E0118"),
        "a bare name in == Val(name) remains a no-shadowing binder: {rejected:?}");
}

#[test]
fn loop_over_total_function_result_keeps_string_element_type() {
    let src = r#"
enum F {
    Map([String:F])
    fn tree(self) -> Dyn Never! {
        if self == {
            .Map(values) -> {
                o := [String:Dyn]{}
                loop key in keys(values.keys().to_list()) -> o[key] = Dyn.Null
                Dyn.Object(o)
            }
        }
    }
}
fn keys(v: ^[String]) -> [String] Never! { v }
fn run() {}
"#;
    let found = errors(src);
    assert!(found.is_empty(), "{found:?}");
}
