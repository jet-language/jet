//! Loop ownership follows executable back edges, and owned tables infer once.
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::{Lexer, Parser, Syntax};
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::AST::{CFfi, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn errors(src: &str) -> Vec<Diagnostic> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(jet_codegen::Codegen::MIREval::install_mir_bridge);
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
fn consume(value: ^[Int]) -[]> Int {{ value.len() }}
fn run() {{
    current := [1]
    loop i in 0..<3 {{
        print(consume(^current))
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
fn consume(value: ^String) -[]> Int { value.count_bytes() }
fn run() {
    shown :: "1.25"
    count := 0
    #Unsafe("exercise the source block boundary") { count = consume(^shown) }
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
fn rebuild(tree: ^Tree) -> Tree Never! {
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
fn consume(value: ^[Int]) -[]> Int {{ value.len() }}
fn run() {{
    current := [1]
    loop {header} {{ print(consume(^current)) }}
}}
"#);
        let found = errors(&source);
        assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    }
}

#[test]
fn an_owned_value_table_does_not_return_ownership_to_its_subject() {
    let found = errors(r#"
enum Tree { Object([Int]) Text(String) }
fn run() {
    owned := Tree.Object([1])
    result := if ^owned == {
        .Object(pairs) -> pairs.len()
        .Text(text) -> text.count_bytes()
    }
    print(result)
    print(owned)
}
"#);
    assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
    assert!(!found.iter().any(|d| d.code == "E0107"), "{found:?}");
}

#[test]
fn borrowed_pattern_payload_views_keep_the_parameter_owner() {
    for body in [
        "if value == .Object(items) { return Ok(items[0..<items.len()]) }\nreturn Err(.Wrong)",
        "if value == {\n.Object(items) -> return Ok(items[0..<items.len()])\nelse -> return Err(.Wrong)\n}",
        "if value == .Array(items) { return Ok(items[0..<items.len()]) }\nreturn Err(.Wrong)",
        "if value == {\n.Array(items) -> return Ok(items[0..<items.len()])\nelse -> return Err(.Wrong)\n}",
    ] {
        let source = format!(r#"
enum PatternPayload {{ Object([Int]) Array([Int]) Text(String) }}
#Error enum Fault {{ Wrong }}
fn select(value: PatternPayload) -> View<Int> Fault! from value {{ {body} }}
fn run() {{}}
"#);
        let found = errors(&source);
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn owned_pattern_payload_views_still_cannot_escape() {
    for parameter in ["value: ^PatternPayload", "value: PatternPayload"] {
        let local = if parameter.contains('^') { "" } else { "owned := PatternPayload.Object([1])" };
        let subject = if local.is_empty() { "^value" } else { "^owned" };
        let source = format!(r#"
enum PatternPayload {{ Object([Int]) Text(String) }}
#Error enum Fault {{ Wrong }}
fn select({parameter}) -> View<Int> Fault! from value {{
    {local}
    if {subject} == {{
        .Object(items) -> return Ok(items[0..<items.len()])
        else -> return Err(.Wrong)
    }}
}}
fn run() {{}}
"#);
        let found = errors(&source);
        assert!(found.iter().any(|d| d.code == "E2305"), "{found:?}");
    }
}

#[test]
fn projected_reinitialization_restores_only_the_moved_field() {
    let found = errors(r#"
struct Inner { xs: [Int], ys: [Int] }
struct Outer { inner: Inner }
fn restore(value: ^Outer) {
    owned := ^value
    xs := ^owned.inner.xs
    owned.inner.xs = [Int]{}
    _ := owned.inner.xs.len()
    _ := owned.inner.ys.len()
    _ := xs.len()
}
fn run() {}
"#);
    assert!(found.is_empty(), "{found:?}");
    let found = errors(r#"
struct Inner { xs: [Int], ys: [Int] }
struct Outer { inner: Inner }
fn invalid(value: ^Outer) {
    owned := ^value
    inner := ^owned.inner
    owned.inner.xs = [Int]{}
    _ := owned.inner.ys.len()
    _ := inner.xs.len()
}
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0121"), "{found:?}");
}

#[test]
fn nested_write_pattern_payloads_reborrow_the_parent_loan() {
    let found = errors(r#"
enum Value { Struct(name: String, fields: [Field]) List([Value]) Text(String) }
struct Field { value: Value }
fn append(result: &Value, item: ^Value, i: Int, j: Int) {
    if &result == .Struct(_, fields) {
        if &fields[i].value == .Struct(_, metadata_fields) {
            if &metadata_fields[j].value == .List(items) {
                &items.push(^item)
            }
        }
    }
}
fn run() {}
"#);
    assert!(found.is_empty(), "{found:?}");
    let found = errors(r#"
enum Value { Struct(name: String, fields: [Field]) List([Value]) Text(String) }
struct Field { value: Value }
fn invalid(result: &Value, i: Int) {
    if &result == .Struct(_, fields) {
        if &fields[i].value == .List(items) {
            &fields.clear()
            _ := items.len()
        }
    }
}
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0212"), "{found:?}");
}

#[test]
fn nested_owned_tables_use_their_own_subject_type() {
    let found = errors(r#"
enum Value { Present(value: Value) Empty }
enum Capture { Report(value: Int) InternalProblem(value: Int) }
fn capture(value: Value) -> Capture Never! { Capture.Report{value: 1} }
fn inspect(value: Value) {
    if value == {
        .Present(nested) -> {
            captured :: capture(nested)
            if ^captured == {
                .Report(child) -> { _ := child }
                .InternalProblem(problem) -> { _ := problem }
            }
        }
        .Empty -> {}
    }
}
fn run() {}
"#);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn scalar_read_pattern_payloads_do_not_lend_the_owner() {
    let found = errors(r#"
enum Frame { Array(first: Bool, index: U64) Key(heap: U64) Empty }
struct Pull { frames: [Frame], lookahead: U8? }
impl Pull {
    fn consume(&self) -> U8 Never! { U8{0} }
    fn next(&self) -> U64 Never! {
        frame :: self.frames[0]
        if frame == {
            .Array(first, index) -> {
                _ :: &self.consume()
                _ :: &self.frames.pop()
                return if first -> index else -> index + U64{1}
            }
            .Key(heap) -> {
                &self.frames.clear()
                return heap
            }
            .Empty -> { return U64{0} }
        }
    }
    fn byte(&self) -> U8? Never! {
        if self.lookahead == .Val(byte) {
            self.lookahead = None
            return Val(byte)
        }
        None
    }
}
fn run() {}
"#);
    assert!(found.is_empty(), "scalar payloads must be values: {found:?}");
    let found = errors(r#"
enum Frame { Text(String) Empty }
struct Pull { frames: [Frame] }
fn invalid(pull: &Pull) {
    frame :: pull.frames[0]
    if frame == .Text(text) {
        &pull.frames.clear()
        _ :: text.len()
    }
}
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0212"), "text must keep its loan: {found:?}");
    let found = errors(r#"
struct Pull { lookahead: U8? }
fn invalid(pull: &Pull) {
    if &pull.lookahead == .Val(byte) {
        pull.lookahead = None
        _ :: byte
    }
}
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0212"), "write windows must keep their loan: {found:?}");
}

#[test]
fn mutable_view_constructor_payloads_preserve_access_marks() {
    let found = errors(include_str!(
        "../../../tests/run/ownership/mutable_view_extraction_transfers_struct_option_and_enum_loans.jet"
    ));
    assert!(found.is_empty(), "constructor payload marks must survive: {found:?}");
    let found = errors(r#"
enum Choice { First(ViewMut<Int>) }
fn choose(values: [Int]) -> Choice { Choice.First(&values[0..1]) }
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0205"), "read parameter cannot grant a write view: {found:?}");
    let found = errors(r#"
enum Choice { First(ViewMut<Int>) }
fn choose(values: &[Int]) -> Choice { Choice.First(values[0..1]) }
fn run() {}
"#);
    assert!(found.iter().any(|d| d.code == "E0108"), "unmarked slices cannot become write views: {found:?}");
}

#[test]
fn mutable_view_disjoint_carrier_fields_use_their_own_loans() {
    let found = errors(include_str!(
        "../../../tests/run/ownership/runtime_disjoint_views_match_aot_jit_dev_and_interpreter.jet"
    ));
    assert!(found.is_empty(), "disjoint carrier fields are writable: {found:?}");
    let found = errors(r#"
fn run() {
    values := [1, 2, 3, 4]
    parts :: &values.split_write(2) ?? panic("split failed")
    (parts).left[0] = 10
    parts.right[1] = (parts).left[0]
    print(parts.right[1])
}
"#);
    assert!(found.is_empty(), "parentheses preserve the projected carrier: {found:?}");
    let found = errors(r#"
fn run() {
    values := [1, 2, 3, 4]
    parts :: &values.split_write(2) ?? panic("split failed")
    print(values[0])
    print(parts.left[0])
}
"#);
    assert!(found.iter().any(|d| d.code == "E0220"), "owner remains suspended beside its field view: {found:?}");
    let found = errors(r#"
fn run() {
    values := [1, 2, 3, 4]
    parts :: &values.split_write(2) ?? panic("split failed")
    child :: &parts.left[0..1]
    parts.left[0] = 10
    print(child[0])
}
"#);
    assert!(found.iter().any(|d| d.code == "E0212"), "a live descendant still suspends its parent field: {found:?}");
}
