//! D-BPE-* Canvas prototype tests: source-backed graph, formatter round-trip,
//! and initial write transactions.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_canvas_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fixture(tag: &str, src: &str) -> PathBuf {
    let dir = temp_dir(tag);
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    path
}

fn ast_enum_variants(path: &str, enum_name: &str) -> Vec<String> {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let marker = format!("enum {enum_name}");
    let mut variants = Vec::new();
    let mut in_enum = false;
    let mut depth = 0i32;
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("");
        if !in_enum {
            if let Some(pos) = code.find(&marker) {
                in_enum = true;
                for ch in code[pos..].chars() {
                    if ch == '{' {
                        depth += 1;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
            }
            continue;
        }
        if depth == 1 {
            if let Some(name) = variant_name(code) {
                variants.push(name.to_string());
            }
        }
        for ch in code.chars() {
            match ch {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0 {
            break;
        }
    }
    assert!(!variants.is_empty(), "missing {marker} variants in {path}");
    variants
}

fn variant_name(chunk: &str) -> Option<&str> {
    let line = chunk
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("///")
                && !line.starts_with("//")
                && !line.starts_with("#[")
        })
        .find(|line| line.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))?;
    let end = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(line.len());
    Some(&line[..end])
}

const CANVAS_FIXTURE: &str = r#"fn square(n: Int) -> Int {
    return n * n
}

fn summarize(limit: Int) -> Int {
    total := square(limit)
    if total > 10 {
        return total
    } else {
        return total + 1
    }
}

fn run() {
    print(summarize(4))
}
"#;

const CANVAS_COVERAGE_FIXTURE: &str = r#"fn coverage(limit: Int) -> Int {
    total := 0
    loop i := 0; i < limit; i++ {
        if i == 2 {
            continue
        }
        total += i
    }
    if total == {
        0 -> { return 1 }
        else -> { return total }
    }
}

fn run() {
    print(coverage(4))
}
"#;

const CANVAS_DATA_FIXTURE: &str = r#"struct Point {
    x: Int
    y: Int
}

enum Choice {
    Pick(Int)
    Skip
}

fn make(n: Int) -> Int {
    p :: Point.{x: n, y: n + 1}
    c :: Choice.Pick(p.x)
    return p.y
}

fn run() {
    print(make(3))
}
"#;

const CANVAS_PIN_AUTHORING_FIXTURE: &str = r#"fn to_int(n: Int) -> Int {
    return n
}

fn choose(limit: Int) -> Int {
    if limit > 1 {
        return limit
    } else {
        return 0
    }
}

fn run() {
    print(choose(3))
}
"#;

const CANVAS_WIRE_FIXTURE: &str = r#"fn pick(a: Int, b: Int) -> Int {
    return a
}

fn run() {
    print(pick(1, 2))
}
"#;

const CANVAS_REORDER_FIXTURE: &str = r#"fn order() {
    a :: 1
    b :: 2
    c :: 3
    print(a + b + c)
}

fn run() {
    order()
}
"#;

const CANVAS_REORDER_BINDING_FIXTURE: &str = r#"fn order() {
    a :: 1
    b :: 2
    c :: a + b
    print(c)
}

fn run() {
    order()
}
"#;

const CANVAS_REORDER_CROSS_BLOCK_FIXTURE: &str = r#"fn order() {
    a :: 1
    if true {
        c :: 3
    } else {
        print(a)
    }
    b :: 2
    print(a + b)
}

fn run() {
    order()
}
"#;

const CANVAS_PATTERN_MULTI_FIXTURE: &str = r#"fn first_or_zero(x: Int?) -> Int {
    if x == Val(n) {
        return n
    } else {
        return 0
    }
}

fn list_total() -> Int {
    xs :: [1, 2, 3]
    ys :: to_int.[1, 2]
    return xs[0] + ys[0]
}

fn to_int(n: Int) -> Int {
    return n
}

fn run() {
    print(first_or_zero(Val(4)))
    print(list_total())
}
"#;

const CANVAS_FUNCTION_EVENT_FIXTURE: &str = r#"/// Starts the scene.
pub fn on_start(limit: Int = 1) -> Int {
    total := limit + 1
    return total
}

fn run() {
    print(on_start())
}
"#;

const CANVAS_COMMENT_FIXTURE: &str = r#"fn run() {
    print("damage")
}
"#;

#[test]
fn canvas_parity_matrix_tracks_ast_language_forms() {
    let matrix = fs::read_to_string("docs/reference/canvas-parity.md")
        .expect("Canvas parity matrix must exist");
    for (enum_name, path) in [
        ("Item", "crates/jet-foundation/src/AST/items.rs"),
        ("Stmt", "crates/jet-foundation/src/AST/statements.rs"),
        ("Expr", "crates/jet-foundation/src/AST/expressions.rs"),
        ("Type", "crates/jet-foundation/src/AST/types.rs"),
        ("Pattern", "crates/jet-foundation/src/AST/patterns.rs"),
        ("BindPattern", "crates/jet-foundation/src/AST/patterns.rs"),
        ("LValue", "crates/jet-foundation/src/AST/lvalues.rs"),
    ] {
        for variant in ast_enum_variants(path, enum_name) {
            let token = format!("[{enum_name}::{variant}]");
            assert!(
                matrix.contains(&token),
                "Canvas parity matrix missing {token} from {path}"
            );
        }
    }
}

const CANVAS_COLLAPSE_FIXTURE: &str = r#"fn compute(limit: Int) -> Int {
    return limit + 1
}

fn run() {
    print(compute(4))
}
"#;

const CANVAS_STRUCTURAL_WRITE_FIXTURE: &str = r#"fn run() -> Void ? {
    print("start")
}
"#;

const CANVAS_RAILS_FIXTURE: &str = r#"use core.mem

fn maybe() -> Int ? String {
    return ok(1)
}

fn checked() -> Int ? String {
    n :: maybe()?
    return ok(n)
}

fn run() -> Void ? {
    #Unsafe("Canvas proof rail fixture") {
        marker := 1
    }
    print(checked()?)
}
"#;

const CANVAS_SHIELD_FIXTURE: &str = r#"fn run() {
    #Shield {
        print("before")
    }
}
"#;

const CANVAS_TASK_RAIL_FIXTURE: &str = r#"fn worker() -> Int {
    return 1
}

fn run() {
    taskgroup g {
        t :: g.task(() => {
            worker()
        })
        result :: g.all([t])
        print(result[0])
    }
}
"#;

const CANVAS_EVENT_DISPATCHER_FIXTURE: &str = r#"use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()
    clicked.on(scope, (n) => { print("clicked {n}") })
    clicked.once(scope, (n) => { print("once {n}") })
    clicked.on_priority(scope, 10, (n) => { print("priority {n}") })
    print(clicked.emit(1).summary())
}
"#;

const CANVAS_TRAIT_INTERFACE_FIXTURE: &str = r#"trait Drawable {
    fn render(self) -> String
}

struct Badge {
    label: String
}

fn run() {
    print("ready")
}
"#;

const CANVAS_TASK_FLOW_FIXTURE: &str = r#"use core.tasks as tasks

fn work() -> Int {
    return 1
}

fn run() {
    (sender, ch) :: tasks.channel<Int>()
    taskgroup g {
        t :: g.task(take(sender) () => {
            sender.send(work())
        })
        g.all([t])
        print(ch.receive() ?? panic("channel closed"))
    }
}
"#;

const CANVAS_DEBUG_FIXTURE: &str = r#"fn run() {
    total := 1
    print(total)
}
"#;

fn field_before(haystack: &str, marker: &str, field: &str) -> String {
    let end = haystack.find(marker).expect("marker in graph JSON");
    let prefix = &haystack[..end];
    let key = format!("\"{field}\":\"");
    let start = prefix.rfind(&key).expect("field before marker") + key.len();
    let rest = &prefix[start..];
    rest[..rest.find('"').expect("field terminator")].to_string()
}

fn first_source_wire_id(graph: &str) -> String {
    for chunk in graph.split("\"wire_id\":\"").skip(1) {
        if chunk.contains("\"source_span\":{\"") {
            return chunk[..chunk.find('"').expect("wire id terminator")].to_string();
        }
    }
    panic!("source-backed wire in graph JSON");
}

fn first_node_id_containing(graph: &str, needle: &str) -> String {
    for chunk in graph.split("\"node_id\":\"").skip(1) {
        let id = &chunk[..chunk.find('"').expect("node id terminator")];
        if id.contains(needle) {
            return id.to_string();
        }
    }
    panic!("node id containing {needle}");
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn json_field(haystack: &str, field: &str) -> String {
    let key = format!("\"{field}\":\"");
    let start = haystack.find(&key).expect("json field") + key.len();
    let rest = &haystack[start..];
    rest[..rest.find('"').expect("json field terminator")].to_string()
}

fn source_span_near(haystack: &str, marker: &str) -> (usize, usize) {
    let pos = haystack.find(marker).expect("marker near source span");
    let rest = &haystack[pos..];
    let key = "\"source_span\":{\"start\":";
    let start_pos = rest.find(key).expect("source span after marker") + key.len();
    let rest = &rest[start_pos..];
    let start = rest[..rest.find(',').expect("span comma")]
        .parse::<usize>()
        .expect("span start");
    let end_key = "\"end\":";
    let end_pos = rest.find(end_key).expect("span end") + end_key.len();
    let rest = &rest[end_pos..];
    let end_text = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    let end = end_text.parse::<usize>().expect("span end number");
    (start, end)
}

fn graph_id_for_title(graph: &str, title: &str) -> String {
    field_before(graph, &format!("\"title\":\"{title}\""), "graph_id")
}

fn name_span(src: &str, name: &str) -> (usize, usize) {
    let start = src.find(&format!("{name} ::")).expect("binding name span");
    (start, start + name.len())
}

fn source_order(src: &str, names: &[&str]) -> Vec<usize> {
    names
        .iter()
        .map(|name| src.find(&format!("{name} ::")).expect("source name"))
        .collect()
}

#[test]
fn canvas_graph_json_is_stable_and_typed() {
    let path = write_fixture("graph", CANVAS_FIXTURE);
    let json = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    assert!(json.contains("\"protocol\":\"jet.canvas.graph\""));
    assert!(json.contains("\"schema_version\":1"));
    assert!(json.contains("\"source_id\""));
    assert!(json.contains("\"fmt_fingerprint\":\"sha256-"));
    assert!(json.contains("\"graph_id\":\"fn:"));
    assert!(json.contains("\"title\":\"summarize\""));
    assert!(json.contains("\"kind\":\"branch\""));
    assert!(json.contains("\"kind\":\"function\""));
    assert!(json.contains("\"archetype\":\"entry\""));
    assert!(json.contains("\"archetype\":\"control\""));
    assert!(json.contains("\"archetype\":\"function_exec\""));
    assert!(json.contains("\"archetype\":\"function_pure\""));
    assert!(json.contains("\"archetype\":\"value\""));
    assert!(json.contains("\"kind\":\"variable_get\""));
    assert!(json.contains("\"kind\":\"constant\""));
    assert!(json.contains("\"type\":\"Int\""));
    assert!(json.contains("\"wire_kind\":\"data\""));
    assert!(json.contains("\"wire_kind\":\"control\""));
    assert!(json.contains("\"from_source_span\":"));
    assert!(json.contains("\"to_source_span\":"));
    assert!(json.contains("\"type\":\"exec\""));
    assert!(json.contains("\"capability\":\"control\""));
    assert!(json.contains("\"inline_exprs\""));
    assert!(json.contains("total > 10"));
    let square_call = first_node_id_containing(&json, ":call:square");
    let binding = first_node_id_containing(&json, ":stmt:1:binding");
    assert!(
        json.contains(&format!(
            "\"from_pin\":\"{square_call}:output:result\",\"to_pin\":\"{binding}:input:value\",\"wire_kind\":\"data\""
        )),
        "pure function calls should feed data pins: {json}"
    );
    assert!(
        !json.contains(&format!("\"pin_id\":\"{square_call}:input:exec\"")),
        "pure expression call must not carry exec pin: {json}"
    );

    let again = jet::Canvas::graph_json_for_file(&path).expect("canvas graph again");
    assert_eq!(
        json, again,
        "Canvas layout/projection must be deterministic"
    );
}

#[test]
fn canvas_noop_round_trip_is_formatter_stable() {
    let path = write_fixture("noop", CANVAS_FIXTURE);
    let before = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let revision = jet::Canvas::source_revision(&before);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"noop\",\"revision\":\"{}\"}}",
        revision
    );

    let out = jet::Canvas::apply_transaction_json(&path, &edit).expect("noop transaction");
    assert!(out.contains("\"changed\":false"), "{out}");
    assert!(out.contains("\"schema_version\":1"), "{out}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    let after_graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph after noop");
    assert_eq!(
        graph, after_graph,
        "no-op write must reproject without drift"
    );
}

#[test]
fn canvas_edit_transactions_write_source_and_reproject() {
    let path = write_fixture("edit", CANVAS_FIXTURE);
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&path).unwrap());
    let rename = format!(
        "{{\"schema_version\":1,\"op\":\"rename_binding\",\"revision\":\"{}\",\"from\":\"total\",\"to\":\"score\"}}",
        revision
    );
    let rename_out =
        jet::Canvas::apply_transaction_json(&path, &rename).expect("rename transaction");
    assert!(rename_out.contains("\"changed\":true"), "{rename_out}");
    let renamed = fs::read_to_string(&path).unwrap();
    assert!(renamed.contains("score := square(limit)"));
    assert!(renamed.contains("if score > 10"));
    assert!(!renamed.contains("total"));

    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph after rename");
    assert!(graph.contains("\"title\":\"score\""));

    let revision = jet::Canvas::source_revision(&renamed);
    let inline_id = field_before(&graph, "\"source\":\"score > 10\"", "inline_expr_id");
    let edit_inline = format!(
        "{{\"schema_version\":1,\"op\":\"edit_inline_expr\",\"revision\":\"{}\",\"inline_expr_id\":\"{}\",\"new_expr\":\"score >= 16\"}}",
        revision, inline_id
    );
    let inline_out =
        jet::Canvas::apply_transaction_json(&path, &edit_inline).expect("inline transaction");
    assert!(inline_out.contains("\"changed\":true"), "{inline_out}");
    let changed = fs::read_to_string(&path).unwrap();
    assert!(changed.contains("if score >= 16"));

    let revision = jet::Canvas::source_revision(&changed);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph before insert");
    let run_graph_id = field_before(&graph, "\"title\":\"run\"", "graph_id");
    let insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"print\",\"args\":[\"\\\"canvas\\\"\"]}}",
        revision, run_graph_id
    );
    let insert_out =
        jet::Canvas::apply_transaction_json(&path, &insert).expect("insert transaction");
    assert!(insert_out.contains("\"changed\":true"), "{insert_out}");
    let inserted = fs::read_to_string(&path).unwrap();
    assert!(inserted.contains("print(\"canvas\")"));
    let final_graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph final");
    assert!(final_graph.contains("\"title\":\"print\""));
}

#[test]
fn canvas_pin_authoring_transactions_write_visible_source() {
    let path = write_fixture("pin_authoring", CANVAS_PIN_AUTHORING_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let cond_id = field_before(&graph, "\"source\":\"limit > 1\"", "inline_expr_id");
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&path).unwrap());
    let promote = format!(
        "{{\"schema_version\":1,\"op\":\"promote_to_binding\",\"revision\":\"{}\",\"inline_expr_id\":\"{}\",\"name\":\"is_large\"}}",
        revision, cond_id
    );
    let promote_out =
        jet::Canvas::apply_transaction_json(&path, &promote).expect("promote transaction");
    assert!(promote_out.contains("\"changed\":true"), "{promote_out}");
    let promoted = fs::read_to_string(&path).unwrap();
    assert!(promoted.contains("is_large :: limit > 1"));
    assert!(promoted.contains("if is_large"));

    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph after promote");
    let literal_id = field_before(&graph, "\"source\":\"0\"", "inline_expr_id");
    let revision = jet::Canvas::source_revision(&promoted);
    let convert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_visible_conversion\",\"revision\":\"{}\",\"inline_expr_id\":\"{}\",\"callee\":\"to_int\"}}",
        revision, literal_id
    );
    let convert_out =
        jet::Canvas::apply_transaction_json(&path, &convert).expect("conversion transaction");
    assert!(convert_out.contains("\"changed\":true"), "{convert_out}");
    let converted = fs::read_to_string(&path).unwrap();
    assert!(
        converted.contains("return to_int(0)"),
        "visible conversion must be ordinary source: {converted}"
    );
    let final_graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph final");
    assert!(final_graph.contains("\"title\":\"to_int\""));
}

#[test]
fn canvas_link_transactions_break_and_move_source_wires() {
    let break_path = write_fixture("break_link", CANVAS_WIRE_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&break_path).expect("canvas graph");
    let wire_id = first_source_wire_id(&graph);
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&break_path).unwrap());
    let break_req = format!(
        "{{\"schema_version\":1,\"op\":\"break_link\",\"revision\":\"{}\",\"wire_id\":\"{}\"}}",
        revision, wire_id
    );
    let break_out =
        jet::Canvas::apply_transaction_json(&break_path, &break_req).expect("break link");
    assert!(break_out.contains("\"changed\":true"), "{break_out}");
    let broken = fs::read_to_string(&break_path).unwrap();
    assert!(broken.contains("return #Todo"), "{broken}");

    let move_path = write_fixture("move_link", CANVAS_WIRE_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&move_path).expect("canvas graph");
    let wire_id = first_source_wire_id(&graph);
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&move_path).unwrap());
    let move_req = format!(
        "{{\"schema_version\":1,\"op\":\"move_link\",\"revision\":\"{}\",\"wire_id\":\"{}\",\"replacement\":\"b\"}}",
        revision, wire_id
    );
    let move_out = jet::Canvas::apply_transaction_json(&move_path, &move_req).expect("move link");
    assert!(move_out.contains("\"changed\":true"), "{move_out}");
    let moved = fs::read_to_string(&move_path).unwrap();
    assert!(moved.contains("return b"), "{moved}");

    let wrong_path = write_fixture("wrong_link", CANVAS_WIRE_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&wrong_path).expect("canvas graph");
    let wire_id = first_source_wire_id(&graph);
    let before = fs::read_to_string(&wrong_path).unwrap();
    let revision = jet::Canvas::source_revision(&before);
    let wrong_req = format!(
        "{{\"schema_version\":1,\"op\":\"move_link\",\"revision\":\"{}\",\"wire_id\":\"{}\",\"replacement\":\"missing\"}}",
        revision, wire_id
    );
    let err = jet::Canvas::apply_transaction_json(&wrong_path, &wrong_req).unwrap_err();
    assert!(err.contains("\"kind\":\"diagnostic\""), "{err}");
    assert!(err.contains("Error [E0107]"), "{err}");
    assert_eq!(fs::read_to_string(&wrong_path).unwrap(), before);
}

#[test]
fn canvas_exec_rewire_reorders_statements_and_guards_scope_and_dataflow() {
    let path = write_fixture("reorder_exec", CANVAS_REORDER_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let graph_id = graph_id_for_title(&graph, "order");
    assert!(
        graph.contains("\"wire_kind\":\"control\"")
            && graph.contains("\"from_source_span\":{\"start\":")
            && graph.contains("\"to_source_span\":{\"start\":"),
        "control wires must carry endpoint statement spans: {graph}"
    );
    let src = fs::read_to_string(&path).unwrap();
    let (moved_start, moved_end) = name_span(&src, "c");
    let (anchor_start, anchor_end) = name_span(&src, "a");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"reorder_statements\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"moved_start\":{},\"moved_end\":{},\"anchor_start\":{},\"anchor_end\":{},\"position\":\"after\"}}",
        jet::Canvas::source_revision(&src),
        graph_id,
        moved_start,
        moved_end,
        anchor_start,
        anchor_end
    );
    jet::Canvas::apply_transaction_json(&path, &req).expect("reorder statements");
    let after = fs::read_to_string(&path).unwrap();
    let order = source_order(&after, &["a", "c", "b"]);
    assert!(order[0] < order[1] && order[1] < order[2], "{after}");
    let formatted = jet::format_source(&after).expect("format reordered source");
    assert_eq!(after, formatted, "reorder must be formatter-stable");

    let cross = write_fixture("reorder_cross_block", CANVAS_REORDER_CROSS_BLOCK_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&cross).expect("cross graph");
    let graph_id = graph_id_for_title(&graph, "order");
    let src = fs::read_to_string(&cross).unwrap();
    let (moved_start, moved_end) = name_span(&src, "c");
    let (anchor_start, anchor_end) = name_span(&src, "a");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"reorder_statements\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"moved_start\":{},\"moved_end\":{},\"anchor_start\":{},\"anchor_end\":{},\"position\":\"after\"}}",
        jet::Canvas::source_revision(&src),
        graph_id,
        moved_start,
        moved_end,
        anchor_start,
        anchor_end
    );
    let err = jet::Canvas::apply_transaction_json(&cross, &req).unwrap_err();
    assert!(
        err.contains("can't move a step into a different branch yet"),
        "{err}"
    );
    assert_eq!(fs::read_to_string(&cross).unwrap(), src);

    let binding = write_fixture("reorder_binding_order", CANVAS_REORDER_BINDING_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&binding).expect("binding graph");
    let graph_id = graph_id_for_title(&graph, "order");
    let src = fs::read_to_string(&binding).unwrap();
    let (moved_start, moved_end) = name_span(&src, "c");
    let (anchor_start, anchor_end) = name_span(&src, "a");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"reorder_statements\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"moved_start\":{},\"moved_end\":{},\"anchor_start\":{},\"anchor_end\":{},\"position\":\"after\"}}",
        jet::Canvas::source_revision(&src),
        graph_id,
        moved_start,
        moved_end,
        anchor_start,
        anchor_end
    );
    let err = jet::Canvas::apply_transaction_json(&binding, &req).unwrap_err();
    assert!(err.contains("\"kind\":\"diagnostic\""), "{err}");
    assert!(err.contains("E0107"), "{err}");
    assert!(err.contains("`b`"), "{err}");
    assert_eq!(fs::read_to_string(&binding).unwrap(), src);
}

#[test]
fn canvas_projection_dedupes_variable_getters_with_fanout() {
    let path = write_fixture(
        "getter_fanout",
        r#"fn run() {
    x :: 1
    print(x)
    print(x)
    print(x)
}
"#,
    );
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    assert_eq!(
        count_occurrences(&graph, "\"kind\":\"variable_get\""),
        1,
        "{graph}"
    );
    let getter_pin_chunk = graph
        .split("\"pin_id\":\"")
        .skip(1)
        .find(|chunk| chunk.contains(":value:get:x:output:x"))
        .expect("getter output pin");
    let getter_pin = getter_pin_chunk[..getter_pin_chunk.find('"').unwrap()].to_string();
    assert_eq!(
        count_occurrences(&graph, &format!("\"from_pin\":\"{getter_pin}\"")),
        3,
        "{graph}"
    );
}

#[test]
fn canvas_projects_pattern_arm_and_multi_input_pin_metadata() {
    let path = write_fixture("pattern_multi", CANVAS_PATTERN_MULTI_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"role\":\"arm\"",
        "\"pattern_source\":\"== Val(n)\"",
        "\"pattern_source_span\":",
        "\"name\":\"arm1\"",
        "\"name\":\"else\"",
        "\"add_pattern_arm\"",
        "\"title\":\"list\"",
        "\"title\":\"fanout\"",
        "\"append_multi_input\"",
        "\"append_op\":\"remove_multi_input_element\"",
        "\"name\":\"item1\"",
        "\"name\":\"item2\"",
        "\"name\":\"item3\"",
    ] {
        assert!(
            graph.contains(field),
            "pattern/multi-input graph missing {field}: {graph}"
        );
    }
}

#[test]
fn canvas_pattern_arm_and_multi_input_transactions_write_source() {
    let path = write_fixture(
        "pattern_arm_tx",
        r#"enum Choice {
    A(Int)
    B(Int)
    C(Int)
}

fn choose(x: Choice) -> Int {
    if x == {
        A(n) -> { return n }
        else -> { return 0 }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
"#,
    );
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let graph_id = graph_id_for_title(&graph, "choose");
    let (node_start, node_end) = source_span_near(&graph, "\"title\":\"if ==\"");
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&path).unwrap());
    let add = format!(
        "{{\"schema_version\":1,\"op\":\"add_pattern_arm\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"node_start\":{},\"node_end\":{},\"pattern\":\"== B(n)\"}}",
        revision, graph_id, node_start, node_end
    );
    let out = jet::Canvas::apply_transaction_json(&path, &add).expect("add arm");
    assert!(out.contains("\"changed\":true"), "{out}");
    let added = fs::read_to_string(&path).unwrap();
    assert!(added.contains("B(n) ->"), "{added}");
    assert!(
        added.contains("return 1"),
        "fresh arm uses sema-safe default return body: {added}"
    );

    let graph = jet::Canvas::graph_json_for_file(&path).expect("graph after add");
    let (pat_start, pat_end) = source_span_near(&graph, "\"pattern_source\":\"B(n)\"");
    let revision = jet::Canvas::source_revision(&added);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"edit_pattern_arm\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"pattern_start\":{},\"pattern_end\":{},\"pattern\":\"== C(n)\"}}",
        revision, graph_id, pat_start, pat_end
    );
    jet::Canvas::apply_transaction_json(&path, &edit).expect("edit arm");
    let edited = fs::read_to_string(&path).unwrap();
    assert!(edited.contains("C(n) ->"), "{edited}");
    assert!(!edited.contains("B(n) ->"), "{edited}");

    let graph = jet::Canvas::graph_json_for_file(&path).expect("graph after edit");
    let (pat_start, pat_end) = source_span_near(&graph, "\"pattern_source\":\"C(n)\"");
    let revision = jet::Canvas::source_revision(&edited);
    let remove = format!(
        "{{\"schema_version\":1,\"op\":\"remove_pattern_arm\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"pattern_start\":{},\"pattern_end\":{}}}",
        revision, graph_id, pat_start, pat_end
    );
    jet::Canvas::apply_transaction_json(&path, &remove).expect("remove arm");
    let removed = fs::read_to_string(&path).unwrap();
    assert!(!removed.contains("C(n) ->"), "{removed}");

    let invalid_path = write_fixture(
        "pattern_arm_invalid_tx",
        r#"enum Choice {
    A(Int)
}

fn choose(x: Choice) -> Int {
    if x == {
        A(n) -> { return n }
    }
}

fn run() {
    print(choose(Choice.A(1)))
}
"#,
    );
    let graph = jet::Canvas::graph_json_for_file(&invalid_path).expect("invalid graph");
    let graph_id = graph_id_for_title(&graph, "choose");
    let (pat_start, pat_end) = source_span_near(&graph, "\"pattern_source\":\"A(n)\"");
    let before = fs::read_to_string(&invalid_path).unwrap();
    let revision = jet::Canvas::source_revision(&before);
    let remove_last = format!(
        "{{\"schema_version\":1,\"op\":\"remove_pattern_arm\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"pattern_start\":{},\"pattern_end\":{}}}",
        revision, graph_id, pat_start, pat_end
    );
    let err = jet::Canvas::apply_transaction_json(&invalid_path, &remove_last).unwrap_err();
    assert!(err.contains("can't remove the last pattern arm"), "{err}");
    assert_eq!(fs::read_to_string(&invalid_path).unwrap(), before);

    let multi_path = write_fixture(
        "multi_input_tx",
        r#"fn to_int(n: Int) -> Int {
    return n
}

fn demo() -> Int {
    xs :: [1, 2, 3]
    ys :: to_int.[1, 2]
    return xs[0] + ys[0]
}

fn run() {
    print(demo())
}
"#,
    );
    let graph = jet::Canvas::graph_json_for_file(&multi_path).expect("multi graph");
    let (list_start, list_end) = source_span_near(&graph, "\"title\":\"list\"");
    let revision = jet::Canvas::source_revision(&fs::read_to_string(&multi_path).unwrap());
    let append = format!(
        "{{\"schema_version\":1,\"op\":\"append_multi_input\",\"revision\":\"{}\",\"node_start\":{},\"node_end\":{},\"element\":\"4\"}}",
        revision, list_start, list_end
    );
    jet::Canvas::apply_transaction_json(&multi_path, &append).expect("append list");
    let appended = fs::read_to_string(&multi_path).unwrap();
    assert!(appended.contains("[1, 2, 3, 4]"), "{appended}");

    let graph = jet::Canvas::graph_json_for_file(&multi_path).expect("multi graph after append");
    let (list_start, list_end) = source_span_near(&graph, "\"title\":\"list\"");
    let (item_start, item_end) = source_span_near(&graph, "\"name\":\"item4\"");
    let revision = jet::Canvas::source_revision(&appended);
    let remove = format!(
        "{{\"schema_version\":1,\"op\":\"remove_multi_input_element\",\"revision\":\"{}\",\"node_start\":{},\"node_end\":{},\"element_start\":{},\"element_end\":{}}}",
        revision, list_start, list_end, item_start, item_end
    );
    jet::Canvas::apply_transaction_json(&multi_path, &remove).expect("remove list item");
    let removed = fs::read_to_string(&multi_path).unwrap();
    assert!(removed.contains("[1, 2, 3]"), "{removed}");
    assert!(!removed.contains("[1, 2, 3, 4]"), "{removed}");

    let graph = jet::Canvas::graph_json_for_file(&multi_path).expect("fanout graph");
    let (fan_start, fan_end) = source_span_near(&graph, "\"title\":\"fanout\"");
    let revision = jet::Canvas::source_revision(&removed);
    let append = format!(
        "{{\"schema_version\":1,\"op\":\"append_multi_input\",\"revision\":\"{}\",\"node_start\":{},\"node_end\":{},\"element\":\"3\"}}",
        revision, fan_start, fan_end
    );
    jet::Canvas::apply_transaction_json(&multi_path, &append).expect("append fanout");
    let fanout = fs::read_to_string(&multi_path).unwrap();
    assert!(fanout.contains("to_int.[1, 2, 3]"), "{fanout}");
}

#[test]
fn canvas_wired_insert_transaction_writes_call_with_origin_value() {
    let path = write_fixture(
        "wired_insert",
        r#"fn use_int(n: Int) {
    print(n)
}

fn demo(x: Int) {
}

fn run() {
    demo(1)
}
"#,
    );
    let src = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let demo_graph_id = field_before(&graph, "\"title\":\"demo\"", "graph_id");
    let revision = jet::Canvas::source_revision(&src);
    let insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"use_int\",\"args\":[],\"wire_origin_pin_id\":\"local:x\",\"wire_target_pin\":\"n\",\"wire_expr\":\"x\"}}",
        revision, demo_graph_id
    );
    let out = jet::Canvas::apply_transaction_json(&path, &insert).expect("wired insert");
    assert!(out.contains("\"changed\":true"), "{out}");
    let changed = fs::read_to_string(&path).unwrap();
    assert!(changed.contains("use_int(x)"), "{changed}");
    let projected = jet::Canvas::graph_json_for_file(&path).expect("canvas graph after insert");
    assert!(projected.contains("\"title\":\"use_int\""), "{projected}");
    assert!(projected.contains("\"wire_kind\":\"data\""), "{projected}");

    let no_arg_path = write_fixture(
        "wired_insert_exec_no_arg",
        r#"fn helper() {
    print("ok")
}

fn run() {
    print("start")
}
"#,
    );
    let no_arg_src = fs::read_to_string(&no_arg_path).unwrap();
    let no_arg_graph = jet::Canvas::graph_json_for_file(&no_arg_path).expect("canvas graph");
    let run_graph_id = field_before(&no_arg_graph, "\"title\":\"run\"", "graph_id");
    let revision = jet::Canvas::source_revision(&no_arg_src);
    let exec_insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"helper\",\"args\":[],\"wire_origin_pin_id\":\"entry:output:then\",\"wire_target_pin\":\"exec\"}}",
        revision, run_graph_id
    );
    let out = jet::Canvas::apply_transaction_json(&no_arg_path, &exec_insert)
        .expect("exec-origin no-arg wired insert");
    assert!(out.contains("\"changed\":true"), "{out}");
    let changed = fs::read_to_string(&no_arg_path).unwrap();
    assert!(changed.contains("helper()"), "{changed}");
    assert!(!changed.contains("helper(1)"), "{changed}");
}

#[test]
fn canvas_comment_regions_round_trip_through_source_comments() {
    let path = write_fixture("comment_region", CANVAS_COMMENT_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let graph_id = field_before(&graph, "\"title\":\"run\"", "graph_id");
    let start = src.find("print").expect("print call");
    let end = start + "print(\"damage\")".len();
    let revision = jet::Canvas::source_revision(&src);
    let create = format!(
        "{{\"schema_version\":1,\"op\":\"create_comment_region\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"start\":{},\"end\":{},\"title\":\"Damage path\",\"color\":\"#2f80ed\",\"alpha\":\"0.25\",\"bounds\":\"10,20,320,140\"}}",
        revision, graph_id, start, end
    );
    let out = jet::Canvas::apply_transaction_json(&path, &create).expect("create comment region");
    assert!(out.contains("\"changed\":true"), "{out}");
    let commented = fs::read_to_string(&path).unwrap();
    assert!(commented.contains("// canvas:comment"), "{commented}");
    assert!(commented.contains("title=\"Damage path\""), "{commented}");

    let graph = jet::Canvas::graph_json_for_file(&path).expect("comment graph");
    for field in [
        "\"kind\":\"comment\"",
        "\"title\":\"Damage path\"",
        "\"color\":\"#2f80ed\"",
        "\"alpha\":\"0.25\"",
        "\"bounds\"",
    ] {
        assert!(
            graph.contains(field),
            "comment graph missing {field}: {graph}"
        );
    }

    let region_id = field_before(&graph, "\"title\":\"Damage path\"", "region_id");
    let revision = jet::Canvas::source_revision(&commented);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"edit_comment_region\",\"revision\":\"{}\",\"region_id\":\"{}\",\"title\":\"Validated damage\",\"color\":\"#22c55e\",\"alpha\":\"0.4\",\"bounds\":\"30,40,360,180\"}}",
        revision, region_id
    );
    jet::Canvas::apply_transaction_json(&path, &edit).expect("edit comment region");
    let edited = fs::read_to_string(&path).unwrap();
    assert!(edited.contains("title=\"Validated damage\""), "{edited}");
    assert!(edited.contains("color=\"#22c55e\""), "{edited}");
    assert!(edited.contains("bounds=(30,40,360,180)"), "{edited}");

    let graph = jet::Canvas::graph_json_for_file(&path).expect("edited comment graph");
    assert!(graph.contains("\"title\":\"Validated damage\""), "{graph}");
    assert!(graph.contains("\"x\":30"), "{graph}");

    let revision = jet::Canvas::source_revision(&edited);
    let delete = format!(
        "{{\"schema_version\":1,\"op\":\"delete_comment_region\",\"revision\":\"{}\",\"region_id\":\"{}\"}}",
        revision, region_id
    );
    jet::Canvas::apply_transaction_json(&path, &delete).expect("delete comment region");
    let deleted = fs::read_to_string(&path).unwrap();
    assert!(!deleted.contains("canvas:comment"), "{deleted}");
    assert!(deleted.contains("print(\"damage\")"), "{deleted}");
}

#[test]
fn canvas_collapse_extract_preview_and_inline_are_source_refactors() {
    let path = write_fixture("collapse_extract", CANVAS_COLLAPSE_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let graph_id = field_before(&graph, "\"title\":\"compute\"", "graph_id");
    let expr_id = field_before(&graph, "\"source\":\"limit + 1\"", "inline_expr_id");
    let start = src.find("return limit + 1").expect("return span");
    let end = start + "return limit + 1".len();
    let revision = jet::Canvas::source_revision(&src);
    let collapse = format!(
        "{{\"schema_version\":1,\"op\":\"create_collapsed_region\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"start\":{},\"end\":{},\"title\":\"Compute body\"}}",
        revision, graph_id, start, end
    );
    jet::Canvas::apply_transaction_json(&path, &collapse).expect("collapse region");
    let collapsed = fs::read_to_string(&path).unwrap();
    assert!(collapsed.contains("// canvas:collapse"), "{collapsed}");
    let graph = jet::Canvas::graph_json_for_file(&path).expect("collapse graph");
    assert!(graph.contains("\"kind\":\"collapse\""), "{graph}");
    let collapse_region = field_before(&graph, "\"title\":\"Compute body\"", "region_id");
    let revision = jet::Canvas::source_revision(&collapsed);
    let expand = format!(
        "{{\"schema_version\":1,\"op\":\"expand_collapsed_region\",\"revision\":\"{}\",\"region_id\":\"{}\"}}",
        revision, collapse_region
    );
    jet::Canvas::apply_transaction_json(&path, &expand).expect("expand collapse region");
    let expanded = fs::read_to_string(&path).unwrap();
    assert!(!expanded.contains("canvas:collapse"), "{expanded}");

    let revision = jet::Canvas::source_revision(&expanded);
    let preview = format!(
        "{{\"schema_version\":1,\"op\":\"preview_extract_inline_expr\",\"revision\":\"{}\",\"inline_expr_id\":\"{}\",\"function\":\"inc_limit\",\"ret_type\":\"Int\"}}",
        revision, expr_id
    );
    let preview_out =
        jet::Canvas::apply_transaction_json(&path, &preview).expect("extract preview");
    assert!(
        preview_out.contains("\"protocol\":\"jet.canvas.preview\""),
        "{preview_out}"
    );
    assert!(
        preview_out.contains("+fn inc_limit(limit: Int) -> Int"),
        "{preview_out}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), expanded);

    let extract = preview.replace("preview_extract_inline_expr", "extract_inline_expr");
    jet::Canvas::apply_transaction_json(&path, &extract).expect("extract inline");
    let extracted = fs::read_to_string(&path).unwrap();
    assert!(
        extracted.contains("fn inc_limit(limit: Int) -> Int"),
        "{extracted}"
    );
    assert!(extracted.contains("return inc_limit(limit)"), "{extracted}");

    let revision = jet::Canvas::source_revision(&extracted);
    let call_start = extracted.find("inc_limit(limit)").expect("helper call");
    let inline = format!(
        "{{\"schema_version\":1,\"op\":\"inline_helper_call\",\"revision\":\"{}\",\"start\":{},\"end\":{}}}",
        revision,
        call_start,
        call_start + "inc_limit(limit)".len()
    );
    jet::Canvas::apply_transaction_json(&path, &inline).expect("inline helper");
    let inlined = fs::read_to_string(&path).unwrap();
    assert!(inlined.contains("return limit + 1"), "{inlined}");
}

#[test]
fn canvas_structural_writes_insert_control_and_fallible_rails_with_undo_source() {
    let path = write_fixture("structural_writes", CANVAS_STRUCTURAL_WRITE_FIXTURE);
    let original = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    assert!(graph.contains("\"source_text\""), "{graph}");
    let graph_id = field_before(&graph, "\"title\":\"run\"", "graph_id");

    for op in [
        "insert_branch",
        "insert_switch",
        "insert_loop",
        "insert_fallible_rail",
    ] {
        let revision = jet::Canvas::source_revision(&fs::read_to_string(&path).unwrap());
        let req = format!(
            "{{\"schema_version\":1,\"op\":\"{}\",\"revision\":\"{}\",\"graph_id\":\"{}\"}}",
            op, revision, graph_id
        );
        let out = jet::Canvas::apply_transaction_json(&path, &req).expect(op);
        assert!(out.contains("\"source_text\""), "{out}");
    }

    let written = fs::read_to_string(&path).unwrap();
    for field in [
        "if true",
        "if 0 ==",
        "loop {",
        "fallible_value: Int ? String :: ok(1)",
        "unwrapped :: fallible_value?",
    ] {
        assert!(written.contains(field), "missing {field}: {written}");
    }
    let graph = jet::Canvas::graph_json_for_file(&path).expect("structural graph");
    for field in [
        "\"kind\":\"branch\"",
        "\"kind\":\"function\"",
        "\"kind\":\"loop\"",
        "\"kind\":\"fallible\"",
        "\"fallible\"",
    ] {
        assert!(
            graph.contains(field),
            "structural graph missing {field}: {graph}"
        );
    }

    let revision = jet::Canvas::source_revision(&written);
    let escaped_original = original
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    let undo = format!(
        "{{\"schema_version\":1,\"op\":\"replace_source\",\"revision\":\"{}\",\"source\":\"{}\"}}",
        revision, escaped_original
    );
    jet::Canvas::apply_transaction_json(&path, &undo).expect("undo source replace");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn canvas_fallible_rail_is_excluded_outside_fallible_function() {
    let path = write_fixture("fallible_rail_nonfallible", "fn run() {\n    print(1)\n}\n");
    let before = fs::read_to_string(&path).unwrap();
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let graph_id = field_before(&graph, "\"title\":\"run\"", "graph_id");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"insert_fallible_rail\",\"revision\":\"{}\",\"graph_id\":\"{}\"}}",
        jet::Canvas::source_revision(&before),
        graph_id
    );
    let err = jet::Canvas::apply_transaction_json(&path, &req).unwrap_err();
    assert!(err.contains("\"kind\":\"unavailable\""), "{err}");
    assert!(err.contains("needs a fallible function"), "{err}");
    assert!(!err.contains("E0403"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn canvas_stale_transaction_conflicts_without_writing() {
    let path = write_fixture("stale", CANVAS_FIXTURE);
    let stale = "{\"schema_version\":1,\"op\":\"rename_binding\",\"revision\":\"sha256-stale\",\"from\":\"total\",\"to\":\"score\"}";
    let err = jet::Canvas::apply_transaction_json(&path, stale).unwrap_err();
    assert!(err.contains("\"kind\":\"conflict\""), "{err}");
    assert!(fs::read_to_string(&path).unwrap().contains("total"));
}

#[test]
fn canvas_code_lens_source_edit_uses_replace_source_transaction() {
    let path = write_fixture("code_lens_edit", "fn run() {\n    print(\"old\")\n}\n");
    let before = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&before);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"replace_source\",\"revision\":\"{}\",\"source\":\"fn run() {{\\n    print(\\\"new\\\")\\n}}\\n\",\"source_edit\":true}}",
        revision
    );
    let out = jet::Canvas::apply_transaction_json(&path, &edit).expect("source edit");
    assert!(out.contains("\"changed\":true"), "{out}");
    assert!(out.contains("print(\\\"new\\\")"), "{out}");
    let graph = jet::Canvas::graph_json_for_file(&path).expect("graph after source edit");
    assert!(graph.contains("print(\\\"new\\\")"), "{graph}");
    assert!(fs::read_to_string(&path).unwrap().contains("print(\"new\")"));
}

#[test]
fn canvas_query_search_references_source_jump_and_rename_preview() {
    let path = write_fixture("query", CANVAS_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);

    let find = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"revision\":\"{}\",\"query\":\"square\"}}",
        revision
    );
    let found = jet::Canvas::query_json_for_file(&path, &find).expect("find query");
    assert!(
        found.contains("\"protocol\":\"jet.canvas.query\""),
        "{found}"
    );
    assert!(found.contains("\"op\":\"find\""), "{found}");
    assert!(found.contains("\"kind\":\"definition\""), "{found}");
    assert!(found.contains("\"graph_id\":\"fn:"), "{found}");
    assert!(found.contains("\"node_id\""), "{found}");

    let refs = format!(
        "{{\"schema_version\":1,\"op\":\"references\",\"revision\":\"{}\",\"symbol\":\"total\"}}",
        revision
    );
    let refs = jet::Canvas::query_json_for_file(&path, &refs).expect("references query");
    assert!(refs.contains("\"kind\":\"definition\""), "{refs}");
    assert!(refs.contains("\"kind\":\"reference\""), "{refs}");
    assert!(refs.contains("\"impact\""), "{refs}");

    let start = src.find("total := square").expect("total binding");
    let jump = format!(
        "{{\"schema_version\":1,\"op\":\"source_to_graph\",\"revision\":\"{}\",\"start\":{},\"end\":{}}}",
        revision,
        start,
        start + "total".len()
    );
    let jump = jet::Canvas::query_json_for_file(&path, &jump).expect("source jump query");
    assert!(jump.contains("\"op\":\"source_to_graph\""), "{jump}");
    assert!(jump.contains(":binding"), "{jump}");

    let preview = format!(
        "{{\"schema_version\":1,\"op\":\"preview_rename\",\"revision\":\"{}\",\"symbol\":\"total\",\"to\":\"score\"}}",
        revision
    );
    let preview = jet::Canvas::query_json_for_file(&path, &preview).expect("rename preview query");
    assert!(preview.contains("\"diff\""), "{preview}");
    assert!(preview.contains("-    total := square(limit)"), "{preview}");
    assert!(preview.contains("+    score := square(limit)"), "{preview}");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("total := square(limit)"),
        "preview must not write"
    );

    let stale =
        "{\"schema_version\":1,\"op\":\"find\",\"revision\":\"sha256-stale\",\"query\":\"total\"}";
    let err = jet::Canvas::query_json_for_file(&path, stale).unwrap_err();
    assert!(err.contains("\"kind\":\"conflict\""), "{err}");
}

#[test]
fn canvas_actions_project_palette_entries_and_preview_jit_backed_source_transactions() {
    let path = write_fixture("actions", CANVAS_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);

    let actions_req = format!(
        "{{\"schema_version\":1,\"op\":\"actions\",\"revision\":\"{}\"}}",
        revision
    );
    let actions = jet::Canvas::query_json_for_file(&path, &actions_req).expect("actions query");
    for field in [
        "\"op\":\"actions\"",
        "\"actions_schema_version\":1",
        "\"kind\":\"canvas.action\"",
        "\"engine\":\"checked-tir+jit\"",
        "\"writes\":\"source_transaction_only\"",
        "\"authority\":[\"canvas.source_edit:single_file\"]",
        "\"package_id\":\"single-file\"",
        "\"version\":\"unpackaged\"",
        "\"project_functions\"",
        "\"name\":\"square\"",
        "\"signature\":\"fn square(n: Int) -> Int\"",
        "\"name\":\"summarize\"",
        "\"signature\":\"fn summarize(limit: Int) -> Int\"",
        "\"name\":\"run\"",
        "\"signature\":\"fn run()\"",
        "\"name\":\"run\",\"signature\":\"fn run()\",\"callee\":\"run\"",
        "\"audit\":[\"package_id\",\"version\",\"hash\",\"authority\",\"touched_files\",\"diff\",\"diagnostics\"]",
        "\"callee\":\"square\"",
        "\"default_args\":[\"1\"]",
        "\"kind\":\"canvas.builtin\"",
        "\"title\":\"Print\"",
        "\"callee\":\"print\"",
        "\"module_path\":\"builtin\"",
        "\"ret\":\"Void\"",
        "\"pins\":[{\"name\":\"value\",\"direction\":\"input\",\"type\":\"Any\"}]",
        "\"default_args\":[\"\\\"canvas\\\"\"]",
        "\"kind\":\"canvas.core_catalog\"",
        "\"engine\":\"checked-tir+jit\"",
        "\"execution\":\"source_transaction\"",
        "\"writes\":\"source_transaction_only\"",
        "\"insert_callee\"",
        "\"insert_op\":\"insert_call\"",
        "\"pure\"",
        "\"source\":\"docs/reference/core-library.md\"",
        "\"kind\":\"canvas.command\"",
        "\"op\":\"command_authority\"",
        "\"engine\":\"jet-cli\"",
        "\"action_id\":\"canvas.command:run\"",
        "\"title\":\"Run program\"",
        "\"command\":[\"jet\",\"run\",\"main.jet\"]",
        "\"authority\":[\"canvas.command:run\",\"canvas.source_edit:single_file\"]",
        "\"action_id\":\"canvas.command:check\"",
        "\"command\":[\"jet\",\"check\",\"main.jet\"]",
        "\"writes\":\"none\"",
        "\"requires_confirmation\":false",
        "\"action_id\":\"canvas.command:build\"",
        "\"authority\":[\"canvas.command:build\",\"canvas.build_output:binary\",\"canvas.source_edit:single_file\"]",
        "\"command\":[\"jet\",\"dev\",\"main.jet\",\"--target=web\"]",
        "\"writes\":\"dev_server\"",
        "\"action_id\":\"canvas.command:service.start\"",
        "\"available\":false",
        "\"denied_reason\":\"no env service selected\"",
        ] {
        assert!(actions.contains(field), "actions missing {field}: {actions}");
    }

    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let run_graph_id = field_before(&graph, "\"title\":\"run\"", "graph_id");
    let preview = format!(
        "{{\"schema_version\":1,\"op\":\"preview_canvas_action\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"action_id\":\"canvas.action:{}:square\",\"callee\":\"square\",\"args\":[\"1\"]}}",
        revision,
        run_graph_id,
        path.display()
    );
    let out = jet::Canvas::apply_transaction_json(&path, &preview).expect("action preview");
    for field in [
        "\"protocol\":\"jet.canvas.action\"",
        "\"engine\":\"checked-tir+jit\"",
        "\"execution\":\"preview\"",
        "\"writes\":\"source_transaction_only\"",
        "\"authority\":[\"canvas.source_edit:single_file\"]",
        "\"package_id\":\"single-file\"",
        "\"touched_files\":[\"main.jet\"]",
        "+    square(1)",
    ] {
        assert!(out.contains(field), "action preview missing {field}: {out}");
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        src,
        "Canvas action preview must not write source"
    );

    let dir = temp_dir("actions_package");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"canvas_actions\", version: \"0.7.0\" }\n",
    )
    .unwrap();
    let pkg_path = dir.join("main.jet");
    fs::write(&pkg_path, CANVAS_FIXTURE).unwrap();
    let pkg_src = fs::read_to_string(&pkg_path).unwrap();
    let pkg_actions_req = format!(
        "{{\"schema_version\":1,\"op\":\"actions\",\"revision\":\"{}\"}}",
        jet::Canvas::source_revision(&pkg_src)
    );
    let pkg_actions =
        jet::Canvas::query_json_for_file(&pkg_path, &pkg_actions_req).expect("package actions");
    for field in [
        "\"authority\":[\"canvas.source_edit:package\"]",
        "\"package_id\":\"canvas_actions\"",
        "\"version\":\"0.7.0\"",
        "\"touched_files\":[\"main.jet\"]",
        "\"command\":[\"jet\",\"build\",\"main.jet\"]",
        "\"authority\":[\"canvas.command:dev\",\"canvas.service:dev_server\",\"canvas.source_edit:package\"]",
    ] {
        assert!(pkg_actions.contains(field), "package actions missing {field}: {pkg_actions}");
    }

    let builtin_preview = format!(
        "{{\"schema_version\":1,\"op\":\"preview_canvas_action\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"action_id\":\"canvas.action:builtin:print\",\"callee\":\"print\",\"args\":[\"\\\"canvas\\\"\"]}}",
        revision, run_graph_id,
    );
    let out = jet::Canvas::apply_transaction_json(&path, &builtin_preview)
        .expect("built-in action preview");
    assert!(out.contains("\"callee\":\"print\""), "{out}");
    assert!(out.contains("+    print(\\\"canvas\\\")"), "{out}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        src,
        "Built-in Canvas action preview must not write source"
    );

    let project_insert_path = write_fixture("actions_project_insert", CANVAS_FIXTURE);
    let project_insert_src = fs::read_to_string(&project_insert_path).unwrap();
    let project_insert_graph =
        jet::Canvas::graph_json_for_file(&project_insert_path).expect("project insert graph");
    let project_run_graph_id =
        field_before(&project_insert_graph, "\"title\":\"run\"", "graph_id");
    let project_insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"square\",\"args\":[\"1\"]}}",
        jet::Canvas::source_revision(&project_insert_src),
        project_run_graph_id,
    );
    let project_insert_out = jet::Canvas::apply_transaction_json(&project_insert_path, &project_insert)
        .expect("project function insert_call");
    assert!(project_insert_out.contains("\"changed\":true"), "{project_insert_out}");
    assert!(
        fs::read_to_string(&project_insert_path)
            .unwrap()
            .contains("square(1)"),
        "project function insert_call should write source"
    );

    let core_path = write_fixture(
        "actions_core_insert",
        "use core.math as math\n\nfn run() {\n    print(1)\n}\n",
    );
    let core_src = fs::read_to_string(&core_path).unwrap();
    let core_revision = jet::Canvas::source_revision(&core_src);
    let core_actions_req = format!(
        "{{\"schema_version\":1,\"op\":\"actions\",\"revision\":\"{}\"}}",
        core_revision
    );
    let core_actions =
        jet::Canvas::query_json_for_file(&core_path, &core_actions_req).expect("core actions");
    assert!(core_actions.contains("\"action_id\":\"canvas.core_catalog:core.math:abs\""), "{core_actions}");
    assert!(core_actions.contains("\"title\":\"abs "), "{core_actions}");
    assert!(core_actions.contains("\"insert_callee\":\"math.abs\""), "{core_actions}");
    let core_graph = jet::Canvas::graph_json_for_file(&core_path).expect("core graph");
    let core_run_graph_id = field_before(&core_graph, "\"title\":\"run\"", "graph_id");
    let core_insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"math.abs\",\"args\":[\"-3\"]}}",
        core_revision, core_run_graph_id,
    );
    let core_out =
        jet::Canvas::apply_transaction_json(&core_path, &core_insert).expect("core insert_call");
    assert!(core_out.contains("\"changed\":true"), "{core_out}");
    assert!(
        fs::read_to_string(&core_path).unwrap().contains("math.abs(-3)"),
        "Core catalog insert_call should write source"
    );

    let synth_path = write_fixture("actions_core_insert_synth_import", "fn run() {\n    print(1)\n}\n");
    let synth_src = fs::read_to_string(&synth_path).unwrap();
    let synth_revision = jet::Canvas::source_revision(&synth_src);
    let synth_graph = jet::Canvas::graph_json_for_file(&synth_path).expect("synth core graph");
    let synth_run_graph_id = field_before(&synth_graph, "\"title\":\"run\"", "graph_id");
    let synth_insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"core.encoding.decode\",\"args\":[\"\\\"canvas\\\"\"]}}",
        synth_revision, synth_run_graph_id,
    );
    let synth_out = jet::Canvas::apply_transaction_json(&synth_path, &synth_insert)
        .expect("core insert_call should synthesize import");
    assert!(synth_out.contains("\"changed\":true"), "{synth_out}");
    assert!(
        synth_out.contains("use core.encoding.json as json"),
        "transaction result should include synthesized Core import: {synth_out}"
    );
    let synth_after = fs::read_to_string(&synth_path).unwrap();
    assert!(synth_after.contains("use core.encoding.json as json"), "{synth_after}");
    assert!(
        synth_after.contains("json.decode(\"canvas\") ?? panic(\"canvas\")"),
        "{synth_after}"
    );
    let synth_reproject = jet::Canvas::graph_json_for_file(&synth_path).expect("synth reproject");
    assert!(
        synth_reproject.contains("\"title\":\".decode\""),
        "reproject should show inserted Core function node: {synth_reproject}"
    );
}

#[test]
fn canvas_core_catalog_browses_canonical_core_library_without_write_authority() {
    let path = write_fixture("core_catalog", "fn run() {\n    print(\"core\")\n}\n");
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let query = format!(
        "{{\"schema_version\":1,\"op\":\"core_catalog\",\"revision\":\"{}\",\"query\":\"\"}}",
        revision
    );
    let catalog =
        jet::Canvas::query_json_for_file(&path, &query).expect("Canvas core catalog query");

    for field in [
        "\"op\":\"core_catalog\"",
        "\"catalog_schema_version\":1",
        "\"authority\":[\"canvas.catalog:core.read\"]",
        "\"writes\":\"none\"",
        "\"source\":\"docs/reference/core-library.md\"",
        "\"path\":\"core.http\"",
        "\"path\":\"core.http.client\"",
        "\"path\":\"core.files\"",
        "\"path\":\"core.url\"",
        "\"path\":\"core.mime\"",
        "\"path\":\"core.crypto\"",
        "\"path\":\"core.event\"",
        "\"path\":\"core.web\"",
        "\"path\":\"core.mem\"",
        "\"signature\":\"Client.request(method, url)\"",
        "\"name\":\"request\"",
        "\"pure\"",
    ] {
        assert!(catalog.contains(field), "catalog missing {field}: {catalog}");
    }

    let sema_query = format!(
        "{{\"schema_version\":1,\"op\":\"core_catalog\",\"revision\":\"{}\",\"query\":\"window_open\"}}",
        revision
    );
    let sema_catalog =
        jet::Canvas::query_json_for_file(&path, &sema_query).expect("Canvas sema catalog query");
    for field in [
        "\"path\":\"core.raylib\"",
        "\"name\":\"window_open\"",
        "\"signature\":\"core.raylib.window_open\"",
        "\"source\":\"crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs\"",
    ] {
        assert!(
            sema_catalog.contains(field),
            "sema catalog missing {field}: {sema_catalog}"
        );
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        src,
        "Core catalog browsing must not edit source"
    );

    let args_query = format!(
        "{{\"schema_version\":1,\"op\":\"core_catalog\",\"revision\":\"{}\",\"query\":\"help\"}}",
        revision
    );
    let args_catalog =
        jet::Canvas::query_json_for_file(&path, &args_query).expect("Canvas args catalog query");
    assert!(args_catalog.contains("\"path\":\"core.args\""), "{args_catalog}");
    assert!(args_catalog.contains("\"name\":\"help\""), "{args_catalog}");
    assert!(args_catalog.contains("\"available\":false"), "{args_catalog}");
    assert!(
        args_catalog.contains("\"unavailable_reason_code\":\"method_only\""),
        "{args_catalog}"
    );
    assert!(
        args_catalog.contains("Use this as a method on an ArgsSpec value."),
        "{args_catalog}"
    );
}

#[test]
fn canvas_projects_function_metadata_and_callback_event_views() {
    let path = write_fixture("function_events", CANVAS_FUNCTION_EVENT_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    for field in [
        "\"title\":\"on_start\"",
        "\"function\":{\"name\":\"on_start\"",
        "\"signature\":\"pub fn on_start(limit: Int = 1) -> Int\"",
        "\"visibility\":\"public\"",
        "\"docs\":\"Starts the scene.\"",
        "\"returns\":\"Int\"",
        "\"params\":[{\"name\":\"limit\",\"type\":\"Int\"",
        "\"default\":true",
        "\"default_source\":\"1\"",
        "\"edit_affordances\":[\"rename_function\",\"edit_function_signature\",\"create_function\",\"source_jump\"]",
        "\"event_views\":[{\"event_id\":\"fn:main.jet::on_start",
        "\"kind\":\"callback_event\"",
        "\"semantics\":\"ordinary_jet_function\"",
        "\"pending_first_class_events\":\"#286\"",
    ] {
        assert!(graph.contains(field), "graph missing {field}: {graph}");
    }
}

#[test]
fn canvas_projects_meta_attribute_on_functions_and_bindings() {
    let path = write_fixture(
        "meta_attribute",
        r#"#Meta(category: "Movement", tunable)
fn run() {
    #Meta(category: "Movement", tunable)
    speed :: 3
    print("{speed}")
}
"#,
    );
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    for field in [
        "\"function\":{\"name\":\"run\"",
        "\"meta\":{\"category\":\"Movement\",\"tunable\":true}",
        "\"kind\":\"binding\"",
        "\"title\":\"speed\"",
    ] {
        assert!(graph.contains(field), "graph missing {field}: {graph}");
    }
}

#[test]
fn canvas_function_transactions_write_source_and_reproject_calls() {
    let path = write_fixture("function_transactions", CANVAS_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let square_graph_id = field_before(&graph, "\"title\":\"square\"", "graph_id");
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);

    let edit_signature = format!(
        "{{\"schema_version\":1,\"op\":\"edit_function_signature\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"signature\":\"fn square(n: Int = 1) -> Int\"}}",
        revision, square_graph_id
    );
    jet::Canvas::apply_transaction_json(&path, &edit_signature).expect("edit signature");
    let after_signature = fs::read_to_string(&path).unwrap();
    assert!(
        after_signature.contains("fn square(n: Int = 1) -> Int"),
        "{after_signature}"
    );

    let revision = jet::Canvas::source_revision(&after_signature);
    let rename = format!(
        "{{\"schema_version\":1,\"op\":\"rename_function\",\"revision\":\"{}\",\"from\":\"square\",\"to\":\"area\"}}",
        revision
    );
    jet::Canvas::apply_transaction_json(&path, &rename).expect("rename function");
    let after_rename = fs::read_to_string(&path).unwrap();
    assert!(
        after_rename.contains("fn area(n: Int = 1) -> Int"),
        "{after_rename}"
    );
    assert!(
        after_rename.contains("total := area(limit)"),
        "{after_rename}"
    );
    let graph = jet::Canvas::graph_json_for_file(&path).expect("reproject graph");
    assert!(graph.contains("\"title\":\"area\""), "{graph}");
    assert!(
        graph.contains("\"kind\":\"function\",\"archetype\":\"function_pure\",\"title\":\"area\""),
        "{graph}"
    );

    let revision = jet::Canvas::source_revision(&after_rename);
    let create = format!(
        "{{\"schema_version\":1,\"op\":\"create_function\",\"revision\":\"{}\",\"name\":\"helper\",\"params\":\"value: Int\",\"ret_type\":\"Int\"}}",
        revision
    );
    jet::Canvas::apply_transaction_json(&path, &create).expect("create function");
    let after_create = fs::read_to_string(&path).unwrap();
    assert!(
        after_create.contains("fn helper(value: Int) -> Int"),
        "{after_create}"
    );
    assert!(after_create.contains("return 1"), "{after_create}");
    let graph = jet::Canvas::graph_json_for_file(&path).expect("created graph");
    assert!(graph.contains("\"title\":\"helper\""), "{graph}");
}

#[test]
fn canvas_source_control_reports_git_text_diff() {
    let dir = temp_dir("source_control");
    if Command::new("git")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("note: skipping canvas_source_control_reports_git_text_diff (need git)");
        return;
    }
    Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .expect("git init");
    Command::new("git")
        .args(["config", "user.email", "canvas@example.invalid"])
        .current_dir(&dir)
        .output()
        .expect("git config email");
    Command::new("git")
        .args(["config", "user.name", "Canvas Test"])
        .current_dir(&dir)
        .output()
        .expect("git config name");
    let path = dir.join("main.jet");
    fs::write(&path, "fn run() {\n    print(\"old\")\n}\n").unwrap();
    Command::new("git")
        .args(["add", "main.jet"])
        .current_dir(&dir)
        .output()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&dir)
        .output()
        .expect("git commit");

    fs::write(&path, "fn run() {\n    print(\"changed\")\n}\n").unwrap();
    let scm = jet::Canvas::source_control_json_for_file(&path);
    assert!(
        scm.contains("\"protocol\":\"jet.canvas.source_control\""),
        "{scm}"
    );
    assert!(scm.contains("\"available\":true"), "{scm}");
    assert!(scm.contains("\"dirty\":true"), "{scm}");
    assert!(scm.contains("+    print(\\\"changed\\\")"), "{scm}");
    assert!(scm.contains("initial"), "{scm}");
}

#[test]
fn canvas_source_control_reports_project_file_set() {
    let dir = temp_dir("source_control_project");
    if Command::new("git")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("note: skipping canvas_source_control_reports_project_file_set (need git)");
        return;
    }
    Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .expect("git init");
    Command::new("git")
        .args(["config", "user.email", "canvas@example.invalid"])
        .current_dir(&dir)
        .output()
        .expect("git config email");
    Command::new("git")
        .args(["config", "user.name", "Canvas Test"])
        .current_dir(&dir)
        .output()
        .expect("git config name");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"canvas_scm\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"old\")\n}\n").unwrap();
    Command::new("git")
        .args(["add", "pkg.jet", "main.jet"])
        .current_dir(&dir)
        .output()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&dir)
        .output()
        .expect("git commit");

    fs::write(&entry, "fn run() {\n    print(\"changed\")\n}\n").unwrap();
    fs::write(dir.join("helper.jet"), "fn helper() -> Int {\n    return 7\n}\n").unwrap();
    let scm = jet::Canvas::source_control_json_for_entry(&entry);
    assert!(
        scm.contains("\"protocol\":\"jet.canvas.source_control\""),
        "{scm}"
    );
    assert!(scm.contains("\"project_revision\":\"sha256-"), "{scm}");
    assert!(scm.contains("\"available\":true"), "{scm}");
    assert!(scm.contains("\"dirty\":true"), "{scm}");
    assert!(scm.contains("\"dirty_files\":2"), "{scm}");
    assert!(scm.contains("\"files\":["), "{scm}");
    assert!(scm.contains("\"path\":\"main.jet\""), "{scm}");
    assert!(scm.contains("\"path\":\"helper.jet\""), "{scm}");
    assert!(scm.contains("+    print(\\\"changed\\\")"), "{scm}");
    assert!(scm.contains("?? helper.jet"), "{scm}");
    assert!(scm.contains("+fn helper() -> Int"), "{scm}");
}

#[test]
fn canvas_proof_lens_reports_revision_check_git_and_missing_receipts() {
    let dir = temp_dir("proof_lens");
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"proof\")\n}\n").unwrap();

    let proof = jet::Canvas::proof_json_for_entry(&entry, None).expect("canvas proof");
    let src = fs::read_to_string(&entry).unwrap();
    assert!(proof.contains("\"protocol\":\"jet.canvas.proof\""), "{proof}");
    assert!(proof.contains("\"schema_version\":1"), "{proof}");
    assert!(proof.contains(&format!(
        "\"revision\":\"{}\"",
        jet::Canvas::source_revision(&src)
    )));
    assert!(proof.contains("\"check\":{\"state\":\"ok\""), "{proof}");
    assert!(proof.contains("\"source_control\":{\"truth\":\"git-text\""), "{proof}");
    assert!(proof.contains("\"command_receipts\":{\"state\":\"missing\""), "{proof}");
    assert!(proof.contains("no Canvas command authority receipt has run"), "{proof}");
    assert!(proof.contains("\"proof\":{\"state\":\"missing\",\"stale\":true"), "{proof}");

    if Command::new("git")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("note: skipping canvas proof dirty-git assertion (need git)");
    } else {
        Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "canvas@example.invalid"])
            .current_dir(&dir)
            .output()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Canvas Test"])
            .current_dir(&dir)
            .output()
            .expect("git config name");
        Command::new("git")
            .args(["add", "main.jet"])
            .current_dir(&dir)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&dir)
            .output()
            .expect("git commit");
        fs::write(&entry, "fn run() {\n    print(\"proof dirty\")\n}\n").unwrap();
        let dirty = jet::Canvas::proof_json_for_entry(&entry, None).expect("dirty proof");
        assert!(dirty.contains("\"available\":true"), "{dirty}");
        assert!(dirty.contains("\"dirty\":true"), "{dirty}");
    }

    let broken = dir.join("broken.jet");
    fs::write(&broken, "fn run() {\n    missing(\n}\n").unwrap();
    let diagnostic = jet::Canvas::proof_json_for_entry(&broken, None).expect("diagnostic proof");
    assert!(
        diagnostic.contains("\"check\":{\"state\":\"diagnostic\""),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("\"diagnostics_count\":"), "{diagnostic}");
}

#[test]
fn canvas_proof_projects_canonical_budget_report_read_only() {
    let dir = temp_dir("proof_budget_projection");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    let entry = dir.join("src/main.jet");
    fs::write(&entry, r#"module perf.package {
    budgets: [Budget.{
        name: "public-api",
        scope: .Package,
        metric: .PublicApiItems,
        comparison: .Absolute,
        limit: .AtMost(10),
    }],
}
pub fn api() {}
fn run() {}
"#).unwrap();
    let out = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_jet"))).current_dir(&dir).args(["budget", "check", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let command = String::from_utf8(out.stdout).unwrap();
    let report_id = command.split("\"report_id\":\"").nth(1).unwrap().split('"').next().unwrap();
    let before = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().count();
    let proof = jet::Canvas::proof_json_for_entry(&entry, None).expect("canvas proof");
    assert!(proof.contains("\"budget_reports\":{\"mode\":\"read_only\""), "{proof}");
    assert!(proof.contains("\"budget_id\":\"package:public-api\""), "{proof}");
    assert!(proof.contains(&format!("\"report_id\":\"{report_id}\"")), "{proof}");
    assert_eq!(fs::read_dir(dir.join(".jet/perf/reports")).unwrap().count(), before, "Canvas measured or wrote a report");
    fs::write(&entry, format!("{}\n// relevant source digest changed\n", fs::read_to_string(&entry).unwrap())).unwrap();
    let stale = jet::Canvas::proof_json_for_entry(&entry, None).expect("stale canvas proof");
    assert!(stale.contains("\"budget_reports\":{\"mode\":\"read_only\",\"reports\":[]"), "{stale}");
    assert_eq!(fs::read_dir(dir.join(".jet/perf/reports")).unwrap().count(), before, "stale Canvas projection refreshed measurement");
}

#[test]
fn canvas_project_json_reports_single_file_without_manifest() {
    let path = write_fixture("project_single", CANVAS_FIXTURE);
    let json = jet::Canvas::project_json_for_entry(&path);
    assert!(json.contains("\"protocol\":\"jet.canvas.project\""), "{json}");
    assert!(json.contains("\"schema_version\":1"), "{json}");
    assert!(json.contains("\"mode\":\"single_file\""), "{json}");
    assert!(json.contains("\"workspace\":null"), "{json}");
    assert!(json.contains("\"packages\":[]"), "{json}");
    assert!(json.contains("\"kind\":\"source\""), "{json}");
    assert!(json.contains("\"state_policy\""), "{json}");
    assert!(json.contains("\"semantic\":\"source\""), "{json}");
}

#[test]
fn canvas_project_json_projects_workspace_packages_and_files() {
    let dir = temp_dir("project_workspace");
    let hello = dir.join("packages/hello");
    let ranker = dir.join("packages/ranker");
    fs::create_dir_all(&hello).unwrap();
    fs::create_dir_all(&ranker).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: find(\"./packages\")\n}\n",
    )
    .unwrap();
    fs::write(
        hello.join("pkg.jet"),
        "payload: {\n    name: \"hello\",\n    version: \"0.1.0\",\n    target: \"web\",\n}\ndeps: {\n    ranker: \"0.1.0\",\n}\npackages: {\n    hello: executable,\n}\n",
    )
    .unwrap();
    fs::write(
        ranker.join("pkg.jet"),
        "payload: {\n    name: \"ranker\",\n    version: \"0.1.0\",\n}\npackages: {\n    ranker: library,\n}\n",
    )
    .unwrap();
    let entry = hello.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"hi\")\n}\n").unwrap();
    fs::write(ranker.join("lib.jet"), "fn score() -> Int {\n    return 1\n}\n").unwrap();

    let json = jet::Canvas::project_json_for_entry(&entry);
    assert!(json.contains("\"protocol\":\"jet.canvas.project\""), "{json}");
    assert!(json.contains("\"mode\":\"workspace\""), "{json}");
    assert!(json.contains("\"workspace\":{\"path\":\"workspace.jet\""), "{json}");
    assert!(json.contains("\"name\":\"hello\""), "{json}");
    assert!(json.contains("\"path\":\"packages/hello\""), "{json}");
    assert!(json.contains("\"manifest\":\"packages/hello/pkg.jet\""), "{json}");
    assert!(json.contains("\"target\":\"web\""), "{json}");
    assert!(json.contains("\"name\":\"ranker\""), "{json}");
    assert!(json.contains("\"targets\":[{\"package\":\"hello\""), "{json}");
    assert!(json.contains("\"package_path\":\"packages/hello\""), "{json}");
    assert!(json.contains("\"manifest\":\"packages/hello/pkg.jet\""), "{json}");
    assert!(json.contains("\"target\":\"executable\""), "{json}");
    assert!(json.contains("\"target\":\"library\""), "{json}");
    assert!(json.contains("\"source\":\"version:0.1.0\""), "{json}");
    assert!(json.contains("\"path\":\"packages/ranker/lib.jet\""), "{json}");
    assert!(json.contains("\"kind\":\"source\""), "{json}");
}

#[test]
fn canvas_project_json_projects_env_services_and_diagnostics() {
    let dir = temp_dir("project_env_services");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"svcapp\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        dir.join("env.jet"),
        "module env.dev {\n    prompt: \"svcapp\",\n    services: { redis: { enable: true, ports: [6380], init: \"redis-server --port 6380\", ready: \"redis-cli -p 6380 ping\" } },\n}\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"svc\")\n}\n").unwrap();
    let json = jet::Canvas::project_json_for_entry(&entry);
    assert!(json.contains("\"envs\":[{\"path\":\"env.jet\""), "{json}");
    assert!(json.contains("\"prompt\":\"svcapp\""), "{json}");
    assert!(json.contains("\"packages\":[]"), "{json}");
    assert!(json.contains("\"services\":[{\"name\":\"redis\""), "{json}");
    assert!(json.contains("\"ports\":[6380]"), "{json}");
    assert!(json.contains("\"ready\":\"redis-cli -p 6380 ping\""), "{json}");
    assert!(json.contains("\"path\":\"env.jet\""), "{json}");
    assert!(json.contains("\"kind\":\"env\""), "{json}");

    fs::write(dir.join("env.jet"), "module dev { env.dev: System.{} }\n").unwrap();
    let json = jet::Canvas::project_json_for_entry(&entry);
    assert!(json.contains("\"diagnostics\":[{\"code\":\"E0966\""), "{json}");
}

#[test]
fn canvas_project_transactions_preview_apply_and_conflict_on_touched_files() {
    let dir = temp_dir("project_txn");
    let app = dir.join("packages/app");
    let logging = dir.join("packages/logging");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&logging).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: find(\"./packages\")\n}\n",
    )
    .unwrap();
    fs::write(
        app.join("pkg.jet"),
        "payload: {\n    name: \"app\",\n    version: \"0.1.0\",\n}\npackages: {\n    app: executable,\n}\n",
    )
    .unwrap();
    fs::write(
        logging.join("pkg.jet"),
        "payload: { name: \"logging\", version: \"0.1.0\" }\npackages: { logging: library }\n",
    )
    .unwrap();
    let entry = app.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"hi\")\n}\n").unwrap();
    fs::write(app.join("helper.jet"), "fn helper() -> Int {\n    return 1\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let manifest_revision = jet::Canvas::source_revision(&fs::read_to_string(app.join("pkg.jet")).unwrap());
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"add_dependency\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"packages/app/pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"packages/app/pkg.jet\",\"name\":\"logging\",\"spec\":\"path@../logging\"}}",
        project_revision, manifest_revision
    );
    let preview = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect("project transaction preview");
    assert!(preview.contains("\"protocol\":\"jet.canvas.project.edit\""), "{preview}");
    assert!(preview.contains("\"preview\":true"), "{preview}");
    assert!(preview.contains("\"writes\":\"preview_only\""), "{preview}");
    assert!(preview.contains("+    logging: path@../logging,"), "{preview}");
    let before_apply = fs::read_to_string(app.join("pkg.jet")).unwrap();
    assert!(!before_apply.contains("logging: path@../logging"), "{before_apply}");

    fs::write(app.join("helper.jet"), "fn helper() -> Int {\n    return 2\n}\n").unwrap();
    let apply = req.replace("\"preview\":true", "\"preview\":false");
    let applied = jet::Canvas::apply_project_transaction_json(&entry, &apply)
        .expect("unrelated project change should not block touched-file-safe apply");
    assert!(applied.contains("\"preview\":false"), "{applied}");
    assert!(applied.contains("\"writes\":\"source_transaction\""), "{applied}");
    let after_apply = fs::read_to_string(app.join("pkg.jet")).unwrap();
    assert!(after_apply.contains("logging: path@../logging"), "{after_apply}");
    assert!(
        jetpack::PackageManifest::parse(&after_apply).is_ok(),
        "{after_apply}"
    );

    let current_project = jet::Canvas::project_json_for_entry(&entry);
    let current_project_revision = json_field(&current_project, "project_revision");
    let stale_touched = format!(
        "{{\"schema_version\":1,\"op\":\"add_dependency\",\"preview\":false,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"packages/app/pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"packages/app/pkg.jet\",\"name\":\"extra\",\"spec\":\"0.2.0\"}}",
        current_project_revision, manifest_revision
    );
    let err = jet::Canvas::apply_project_transaction_json(&entry, &stale_touched)
        .expect_err("stale touched file should conflict");
    assert!(err.contains("\"kind\":\"conflict\""), "{err}");
    assert!(!fs::read_to_string(app.join("pkg.jet")).unwrap().contains("extra"));
}

#[test]
fn canvas_project_transactions_reject_reserved_create_package_path() {
    let dir = temp_dir("project_reserved_path");
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: [\"./packages/app\"]\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\npackages: { app: executable }\n",
    )
    .unwrap();
    let entry = dir.join("packages/app/main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"create_package\",\"preview\":false,\"project_revision\":\"{}\",\"package_path\":\".git/hooks\",\"files\":[{{\"path\":\".git/hooks/pkg.jet\",\"revision\":\"missing\"}},{{\"path\":\".git/hooks/main.jet\",\"revision\":\"missing\"}}],\"name\":\"hooks\",\"target\":\"executable\"}}",
        project_revision
    );
    let err = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect_err("reserved path should be rejected");
    assert!(err.contains("\"kind\":\"bad_request\""), "{err}");
    assert!(
        !dir.join(".git/hooks/pkg.jet").exists(),
        "reserved path write escaped source truth"
    );
}

#[test]
fn canvas_project_transactions_roll_back_when_later_write_fails() {
    let dir = temp_dir("project_rollback");
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: [\"./packages/app\"]\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\npackages: { app: executable }\n",
    )
    .unwrap();
    let entry = dir.join("packages/app/main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();
    fs::create_dir_all(dir.join("packages/tools")).unwrap();
    fs::write(dir.join("packages/tools/nested"), "not a directory").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"create_package\",\"preview\":false,\"project_revision\":\"{}\",\"package_path\":\"packages/tools\",\"entry\":\"packages/tools/nested/main.jet\",\"files\":[{{\"path\":\"packages/tools/pkg.jet\",\"revision\":\"missing\"}},{{\"path\":\"packages/tools/nested/main.jet\",\"revision\":\"missing\"}}],\"name\":\"tools\",\"target\":\"executable\"}}",
        project_revision
    );
    let err = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect_err("second write should fail and rollback first write");
    assert!(err.contains("\"kind\":\"io\""), "{err}");
    assert!(
        !dir.join("packages/tools/pkg.jet").exists(),
        "first project write must be rolled back"
    );
    assert_eq!(
        fs::read_to_string(dir.join("packages/tools/nested")).unwrap(),
        "not a directory"
    );
}

#[test]
fn canvas_project_transactions_remove_dependency() {
    let dir = temp_dir("project_remove_dep");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: {\n    logging: \"0.1.0\",\n    tools: path@../tools,\n}\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();
    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let manifest_revision =
        jet::Canvas::source_revision(&fs::read_to_string(dir.join("pkg.jet")).unwrap());
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"remove_dependency\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"pkg.jet\",\"name\":\"logging\"}}",
        project_revision, manifest_revision
    );
    let preview = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect("remove dependency preview");
    assert!(preview.contains("\"op\":\"remove_dependency\""), "{preview}");
    assert!(preview.contains("-    logging: \\\"0.1.0\\\","), "{preview}");
    assert!(fs::read_to_string(dir.join("pkg.jet")).unwrap().contains("logging"));

    let apply = req.replace("\"preview\":true", "\"preview\":false");
    let applied = jet::Canvas::apply_project_transaction_json(&entry, &apply)
        .expect("remove dependency apply");
    assert!(applied.contains("\"writes\":\"source_transaction\""), "{applied}");
    let manifest = fs::read_to_string(dir.join("pkg.jet")).unwrap();
    assert!(!manifest.contains("logging"), "{manifest}");
    assert!(manifest.contains("tools: path@../tools"), "{manifest}");
    assert!(
        jetpack::PackageManifest::parse(&manifest).is_ok(),
        "{manifest}"
    );
}

#[test]
fn canvas_project_transactions_edit_pkg_field_and_add_target() {
    let dir = temp_dir("project_pkg_fields");
    fs::write(
        dir.join("pkg.jet"),
        "payload: {\n    name: \"app\",\n    version: \"0.1.0\",\n}\npackages: {\n}\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let manifest_revision =
        jet::Canvas::source_revision(&fs::read_to_string(dir.join("pkg.jet")).unwrap());
    let edit_req = format!(
        "{{\"schema_version\":1,\"op\":\"edit_pkg_field\",\"preview\":false,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"pkg.jet\",\"field\":\"version\",\"value\":\"0.2.0\"}}",
        project_revision, manifest_revision
    );
    let edited = jet::Canvas::apply_project_transaction_json(&entry, &edit_req)
        .expect("edit pkg field apply");
    assert!(edited.contains("\"op\":\"edit_pkg_field\""), "{edited}");
    let manifest = fs::read_to_string(dir.join("pkg.jet")).unwrap();
    assert!(manifest.contains("version: \"0.2.0\""), "{manifest}");

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let manifest_revision = jet::Canvas::source_revision(&manifest);
    let target_req = format!(
        "{{\"schema_version\":1,\"op\":\"add_target\",\"preview\":false,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"pkg.jet\",\"revision\":\"{}\"}}],\"manifest\":\"pkg.jet\",\"name\":\"app\",\"target\":\"executable\"}}",
        project_revision, manifest_revision
    );
    let targeted = jet::Canvas::apply_project_transaction_json(&entry, &target_req)
        .expect("add target apply");
    assert!(targeted.contains("\"op\":\"add_target\""), "{targeted}");
    let manifest = fs::read_to_string(dir.join("pkg.jet")).unwrap();
    assert!(manifest.contains("app: executable"), "{manifest}");
    assert!(
        jetpack::PackageManifest::parse(&manifest).is_ok(),
        "{manifest}"
    );
}

#[test]
fn canvas_project_transactions_add_env_service() {
    let dir = temp_dir("project_add_env_service");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"svcapp\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"svc\")\n}\n").unwrap();
    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"add_env_service\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"env.jet\",\"revision\":\"missing\"}}],\"env\":\"env.jet\",\"name\":\"redis\",\"port\":6380,\"init\":\"redis-server --port 6380\",\"ready\":\"redis-cli -p 6380 ping\"}}",
        project_revision
    );
    let preview = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect("add env service preview");
    assert!(preview.contains("\"op\":\"add_env_service\""), "{preview}");
    assert!(preview.contains("\"writes\":\"preview_only\""), "{preview}");
    assert!(preview.contains("redis-server --port 6380"), "{preview}");
    assert!(!dir.join("env.jet").exists());

    let apply = req.replace("\"preview\":true", "\"preview\":false");
    let applied = jet::Canvas::apply_project_transaction_json(&entry, &apply)
        .expect("add env service apply");
    assert!(applied.contains("\"writes\":\"source_transaction\""), "{applied}");
    let env = fs::read_to_string(dir.join("env.jet")).unwrap();
    assert!(env.contains("module env.dev"), "{env}");
    assert!(env.contains("redis:"), "{env}");
    assert!(env.contains("ports: [6380]"), "{env}");
    let project = jet::Canvas::project_json_for_entry(&entry);
    assert!(project.contains("\"services\":[{\"name\":\"redis\""), "{project}");
    assert!(project.contains("\"ports\":[6380]"), "{project}");
}

#[test]
fn canvas_project_transactions_create_package_from_workspace() {
    let dir = temp_dir("project_create_package");
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: find(\"./packages\")\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\npackages: { app: executable }\n",
    )
    .unwrap();
    let entry = dir.join("packages/app/main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"create_package\",\"preview\":true,\"project_revision\":\"{}\",\"package_path\":\"packages/tools\",\"files\":[{{\"path\":\"packages/tools/pkg.jet\",\"revision\":\"missing\"}},{{\"path\":\"packages/tools/main.jet\",\"revision\":\"missing\"}}],\"name\":\"tools\",\"target\":\"executable\"}}",
        project_revision
    );
    let preview = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect("create package preview");
    assert!(preview.contains("\"op\":\"create_package\""), "{preview}");
    assert!(preview.contains("\"writes\":\"preview_only\""), "{preview}");
    assert!(preview.contains("diff -- packages/tools/pkg.jet"), "{preview}");
    assert!(preview.contains("diff -- packages/tools/main.jet"), "{preview}");
    assert!(!dir.join("packages/tools/pkg.jet").exists());

    let apply = req.replace("\"preview\":true", "\"preview\":false");
    let applied = jet::Canvas::apply_project_transaction_json(&entry, &apply)
        .expect("create package apply");
    assert!(applied.contains("\"writes\":\"source_transaction\""), "{applied}");
    let manifest = fs::read_to_string(dir.join("packages/tools/pkg.jet")).unwrap();
    assert!(manifest.contains("name: \"tools\""), "{manifest}");
    assert!(manifest.contains("tools: executable"), "{manifest}");
    assert!(
        jetpack::PackageManifest::parse(&manifest).is_ok(),
        "{manifest}"
    );
    let main = fs::read_to_string(dir.join("packages/tools/main.jet")).unwrap();
    assert!(main.contains("print(\"tools\")"), "{main}");
    let after_project = jet::Canvas::project_json_for_entry(&entry);
    assert!(after_project.contains("\"name\":\"tools\""), "{after_project}");
    assert!(
        after_project.contains("\"path\":\"packages/tools/main.jet\""),
        "{after_project}"
    );
}

#[test]
fn canvas_project_transactions_add_workspace_member() {
    let dir = temp_dir("project_add_workspace_member");
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::create_dir_all(dir.join("packages/tools")).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: [\"./packages/app\"]\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\npackages: { app: executable }\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/tools/pkg.jet"),
        "payload: { name: \"tools\", version: \"0.1.0\" }\npackages: { tools: library }\n",
    )
    .unwrap();
    let entry = dir.join("packages/app/main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    let project_revision = json_field(&project, "project_revision");
    let workspace_revision =
        jet::Canvas::source_revision(&fs::read_to_string(dir.join("workspace.jet")).unwrap());
    let req = format!(
        "{{\"schema_version\":1,\"op\":\"add_workspace_member\",\"preview\":true,\"project_revision\":\"{}\",\"files\":[{{\"path\":\"workspace.jet\",\"revision\":\"{}\"}}],\"workspace\":\"workspace.jet\",\"member_path\":\"packages/tools\"}}",
        project_revision, workspace_revision
    );
    let preview = jet::Canvas::apply_project_transaction_json(&entry, &req)
        .expect("add workspace member preview");
    assert!(preview.contains("\"op\":\"add_workspace_member\""), "{preview}");
    assert!(preview.contains("\"writes\":\"preview_only\""), "{preview}");
    assert!(
        preview.contains("+    members: [\\\"./packages/app\\\", \\\"./packages/tools\\\"]"),
        "{preview}"
    );
    assert!(!fs::read_to_string(dir.join("workspace.jet")).unwrap().contains("tools"));

    let apply = req.replace("\"preview\":true", "\"preview\":false");
    let applied = jet::Canvas::apply_project_transaction_json(&entry, &apply)
        .expect("add workspace member apply");
    assert!(applied.contains("\"writes\":\"source_transaction\""), "{applied}");
    let workspace = fs::read_to_string(dir.join("workspace.jet")).unwrap();
    assert!(workspace.contains("\"./packages/tools\""), "{workspace}");
    assert!(
        jetpack::WorkspaceFile::evaluate(&workspace, &dir).is_ok(),
        "{workspace}"
    );
    let after_project = jet::Canvas::project_json_for_entry(&entry);
    assert!(after_project.contains("\"name\":\"tools\""), "{after_project}");
}

#[test]
fn canvas_project_source_id_selects_file_graph_and_query() {
    let dir = temp_dir("project_source_id");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"source_id\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, "fn run() {\n    print(\"main\")\n}\n").unwrap();
    let helper = dir.join("helper.jet");
    fs::write(&helper, "fn helper() -> Int {\n    return 7\n}\n").unwrap();

    let graph = jet::Canvas::graph_json_for_entry_source(&entry, Some("helper.jet"))
        .expect("helper graph");
    assert!(graph.contains("\"source_id\""), "{graph}");
    assert!(graph.contains("helper.jet"), "{graph}");
    assert!(graph.contains("\"title\":\"helper\""), "{graph}");
    assert!(!graph.contains("\"title\":\"run\""), "{graph}");

    let helper_src = fs::read_to_string(&helper).unwrap();
    let revision = jet::Canvas::source_revision(&helper_src);
    let query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"source_id\":\"helper.jet\",\"revision\":\"{}\",\"query\":\"helper\"}}",
        revision
    );
    let result = jet::Canvas::query_json_for_entry(&entry, &query).expect("helper query");
    assert!(result.contains("\"protocol\":\"jet.canvas.query\""), "{result}");
    assert!(result.contains("\"title\":\"helper\""), "{result}");

    let missing = jet::Canvas::graph_json_for_entry_source(&entry, Some("missing.jet"))
        .expect_err("bad source_id should be rejected");
    assert!(missing.contains("\"kind\":\"not_found\""), "{missing}");
    let missing_query = format!(
        "{{\"schema_version\":1,\"op\":\"find\",\"source_id\":\"missing.jet\",\"revision\":\"{}\",\"query\":\"helper\"}}",
        revision
    );
    let err = jet::Canvas::query_json_for_entry(&entry, &missing_query)
        .expect_err("bad query source_id should be rejected");
    assert!(err.contains("\"kind\":\"not_found\""), "{err}");
}

#[test]
fn canvas_project_source_id_rejects_existing_unprojected_file() {
    let dir = temp_dir("project_source_id_unprojected");
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: [\"./packages/app\"]\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\npackages: { app: executable }\n",
    )
    .unwrap();
    let entry = dir.join("packages/app/main.jet");
    fs::write(&entry, "fn run() {\n    print(\"app\")\n}\n").unwrap();
    fs::write(dir.join("stray.jet"), "fn stray() -> Int {\n    return 1\n}\n").unwrap();

    let project = jet::Canvas::project_json_for_entry(&entry);
    assert!(!project.contains("\"path\":\"stray.jet\""), "{project}");
    let err = jet::Canvas::graph_json_for_entry_source(&entry, Some("stray.jet"))
        .expect_err("existing but unprojected file should be rejected");
    assert!(err.contains("\"kind\":\"not_found\""), "{err}");
}

#[test]
fn canvas_protocol_doc_matches_v1_graph_and_edit_shape() {
    let doc_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/reference/canvas-protocol.md");
    let doc = fs::read_to_string(doc_path).expect("Canvas protocol reference");
    for term in [
        "jet.canvas.project",
        "jet.canvas.project.edit",
        "project_revision",
        "project_root",
        "add_dependency",
        "remove_dependency",
        "edit_pkg_field",
        "add_target",
        "add_env_service",
        "create_package",
        "add_workspace_member",
        "member_path",
        "package_path",
        "missing",
        "touched",
        "preview_only",
        "source_transaction",
        "state_policy",
        "env.jet",
        "services",
        "single_file",
        "package",
        "workspace",
        "jet.canvas.graph",
        "jet.canvas.edit",
        "schema_version",
        "source_id",
        "revision",
        "fmt_fingerprint",
        "graphs",
        "archetype",
        "diagnostics",
        "facts",
        "facts.blueprint",
        "event_dispatchers",
        "interfaces",
        "task_flows",
        "rails",
        "source_text",
        "jet.canvas.debug",
        "debug_overlay",
        "breakpoint_spans",
        "watches",
        "comment_boxes",
        "staged_nodes",
        "jet.canvas.query",
        "source_to_graph",
        "preview_rename",
        "actions",
        "palette_entries",
        "project_functions",
        "jet.canvas.action",
        "preview_canvas_action",
        "canvas.action",
        "checked-tir+jit",
        "source_transaction_only",
        "external adapter",
        "jet.canvas.source_control",
        "dirty",
        "dirty_files",
        "files",
        "history",
        "canvas:comment",
        "noop",
        "rename_binding",
        "edit_inline_expr",
        "create_comment_region",
        "edit_comment_region",
        "delete_comment_region",
        "replace_source",
        "insert_branch",
        "insert_switch",
        "insert_loop",
        "insert_fallible_rail",
        "canvas:collapse",
        "create_collapsed_region",
        "preview_extract_inline_expr",
        "extract_inline_expr",
        "inline_helper_call",
        "insert_call",
        "create_trait_impl",
        "conflict",
    ] {
        assert!(doc.contains(term), "protocol doc missing `{term}`");
    }

    let path = write_fixture("protocol", CANVAS_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    for field in [
        "\"protocol\":\"jet.canvas.graph\"",
        "\"schema_version\":1",
        "\"source_id\"",
        "\"revision\":\"sha256-",
        "\"fmt_fingerprint\":\"sha256-",
        "\"graphs\"",
        "\"diagnostics\"",
        "\"facts\"",
        "\"rails\"",
        "\"semindex_schema_version\"",
        "\"pin_id\"",
        "\"source_span\"",
    ] {
        assert!(graph.contains(field), "graph JSON missing {field}: {graph}");
    }

    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let noop_with_future_field = format!(
        "{{\"schema_version\":1,\"op\":\"noop\",\"revision\":\"{}\",\"future_nonsemantic\":\"client-note\"}}",
        revision
    );
    let out = jet::Canvas::apply_transaction_json(&path, &noop_with_future_field)
        .expect("unknown nonsemantic request fields are ignored");
    assert!(out.contains("\"protocol\":\"jet.canvas.edit\""), "{out}");
    assert!(out.contains("\"changed\":false"), "{out}");

    let unknown_op = format!(
        "{{\"schema_version\":1,\"op\":\"teleport_node\",\"revision\":\"{}\"}}",
        revision
    );
    let err = jet::Canvas::apply_transaction_json(&path, &unknown_op).unwrap_err();
    assert!(err.contains("\"protocol\":\"jet.canvas.edit\""), "{err}");
    assert!(err.contains("\"kind\":\"unsupported\""), "{err}");
}

#[test]
fn canvas_editor_shell_matches_round3_contract() {
    let html = jet::Canvas::canvas_html();
    let js = jet::Canvas::canvas_js();

    assert!(html.contains("<h2>My Canvas</h2>"), "{html}");
    assert!(html.contains("id=\"variables-list\""), "{html}");
    assert!(html.contains("id=\"status-summary\""), "{html}");
    assert!(html.contains("class=\"toolbar-group\""), "{html}");
    assert!(html.contains("class=\"icon-button\""), "{html}");
    assert!(html.contains("id=\"toolbar-search\""), "{html}");
    assert!(html.contains("<svg viewBox=\"0 0 24 24\""), "{html}");
    assert!(html.contains("id=\"trust-summary\""), "{html}");
    assert!(html.contains("id=\"trust-summary\" class=\"project-section dev-only"), "{html}");
    assert!(!html.contains("source-truth"), "{html}");
    assert!(!html.contains("Source truth"), "{html}");
    assert!(!html.contains(">Trust<"), "{html}");

    assert!(js.contains("function syncVariablesList"), "{js}");
    assert!(js.contains("function renderVariableDetails"), "{js}");
    assert!(js.contains("data-project-file"), "{js}");
    assert!(js.contains("function actionInsertsNode"), "{js}");
    assert!(js.contains("toolbarSearch.addEventListener"), "{js}");
    assert!(js.contains("Add connected node"), "{js}");
    assert!(js.contains("Canvas actions"), "{js}");
    assert!(!js.contains("Graph actions"), "{js}");
    assert!(!js.contains("Refused: E0204"), "{js}");
    assert!(!js.contains("Source truth"), "{js}");
    assert!(!js.contains("source-truth"), "{js}");
}

#[test]
fn canvas_javascript_assets_are_independently_syntax_checked_and_ordered() {
    const ASSETS: &[&str] = &[
        "runtime-state.js",
        "editing-history.js",
        "diagnostics-query.js",
        "drawing-palette.js",
        "project-navigation.js",
        "graph-rendering.js",
        "inspector-connections.js",
        "input-events.js",
        "transactions-catalog.js",
        "bootstrap.js",
    ];

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let asset_dir = repo.join("Source/Canvas/js");
    let mut discovered = fs::read_dir(&asset_dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", asset_dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("read Canvas JS asset entry").path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("js"))
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    discovered.sort();
    let mut expected = ASSETS
        .iter()
        .map(|asset| asset.to_string())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(discovered, expected, "Canvas JS asset set drifted");

    let runtime = jet::Canvas::canvas_js();
    let mut previous_end = "(function () {\n".len();
    assert!(runtime.starts_with("(function () {\n"), "{runtime}");
    assert!(runtime.ends_with("})();\n"), "{runtime}");

    for asset in ASSETS {
        let path = asset_dir.join(asset);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        assert!(
            source.starts_with("// ") || source.starts_with("\n// "),
            "{} needs a section comment",
            path.display()
        );
        let position = runtime[previous_end..]
            .find(&source)
            .map(|offset| previous_end + offset)
            .unwrap_or_else(|| panic!("{} missing from assembled runtime", path.display()));
        assert_eq!(
            position, previous_end,
            "{} is out of order or separated by untracked JavaScript",
            path.display()
        );
        previous_end = position + source.len();

        let output = Command::new("node")
            .arg("--check")
            .arg(&path)
            .output()
            .unwrap_or_else(|err| panic!("run node --check for {}: {err}", path.display()));
        assert!(
            output.status.success(),
            "node --check failed for {}:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(previous_end + "})();\n".len(), runtime.len());
    let glue = fs::read_to_string(repo.join("Source/Canvas/js.rs")).expect("read Canvas JS glue");
    assert!(glue.lines().count() < 40, "js.rs must remain assembly glue");
}

#[test]
fn canvas_debug_session_projects_runtime_overlay_to_source_spans() {
    let path = write_fixture("debug_overlay", CANVAS_DEBUG_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let print_start = src.find("print").expect("print call");
    let req = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"commands\":[\"c\"],\"breakpoint_spans\":[\"{}:{}\"],\"watches\":[\"total\"]}}",
        revision,
        print_start,
        print_start + "print".len()
    );

    let out = jet::Canvas::debug_session_json_for_file(&path, &req).expect("debug session");
    for field in [
        "\"protocol\":\"jet.canvas.debug\"",
        "\"ok\":true",
        "\"persistence\":\"local-source-span\"",
        "\"debug_overlay\":\"running\"",
        "\"active_line\":3",
        "\"active_node_id\":\"fn:",
        "\"active_wire_id\":\"fn:",
        "\"breakpoints\"",
        "\"locals\"",
        "\"watches\"",
        "\"name\":\"total\"",
        "\"value\":\"1\"",
        "\"call_stack\"",
        "\"trace\"",
    ] {
        assert!(out.contains(field), "debug overlay missing {field}: {out}");
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), src);
}

#[test]
fn canvas_debug_declines_unsteppable_programs_with_jet_diagnostics() {
    let path = write_fixture("debug_boundary", CANVAS_RAILS_FIXTURE);
    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let req = format!(
        "{{\"schema_version\":1,\"revision\":\"{}\",\"commands\":[\"s\"]}}",
        revision
    );

    let err = jet::Canvas::debug_session_json_for_file(&path, &req).unwrap_err();
    assert!(err.contains("\"protocol\":\"jet.canvas.debug\""), "{err}");
    assert!(err.contains("\"kind\":\"diagnostic\""), "{err}");
    assert!(err.contains("Error [E2203]"), "{err}");
    assert!(!err.contains("rustc"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), src);
}

#[test]
fn canvas_projects_nested_control_and_assignment_forms() {
    let path = write_fixture("coverage", CANVAS_COVERAGE_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"kind\":\"loop\"",
        "\"kind\":\"branch\"",
        "\"kind\":\"assign\"",
        "\"kind\":\"flow\"",
        "\"kind\":\"function\"",
        "\"title\":\"if ==\"",
        "\"title\":\"continue\"",
        "\"source\":\"i < limit\"",
        "\"source\":\"i == 2\"",
        "\"source\":\"total\"",
    ] {
        assert!(
            graph.contains(field),
            "coverage graph missing {field}: {graph}"
        );
    }
    assert!(
        !graph.contains("\"kind\":\"source\""),
        "supported control/assignment forms must not project as opaque source nodes: {graph}"
    );
}

#[test]
fn canvas_projects_data_construction_as_nodes() {
    let path = write_fixture("data", CANVAS_DATA_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"kind\":\"expr\"",
        "\"kind\":\"variant\"",
        "\"title\":\"construct\"",
        "\"title\":\"Pick\"",
        "Point.{x: n, y: n + 1}",
    ] {
        assert!(graph.contains(field), "data graph missing {field}: {graph}");
    }
}

#[test]
fn canvas_projects_control_data_fallible_effect_proof_debug_rails() {
    let path = write_fixture("rails", CANVAS_RAILS_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"rails\"",
        "\"control\"",
        "\"data\"",
        "\"fallible\"",
        "\"effect\"",
        "\"proof\"",
        "\"debug\"",
        "\"debug_overlay\":\"idle\"",
        "\"wire_kind\":\"fallible\"",
        "\"kind\":\"unsafe\"",
    ] {
        assert!(
            graph.contains(field),
            "rails graph missing {field}: {graph}"
        );
    }
}

#[test]
fn canvas_projects_and_source_edits_shield_region() {
    let path = write_fixture("shield_region", CANVAS_SHIELD_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("shield graph");
    for field in [
        "\"kind\":\"shield\"",
        "\"title\":\"#Shield\"",
        "\"source_span\":{\"start\":",
        "\"title\":\"print\"",
        "\"title\":\"\\\"before\\\"\"",
    ] {
        assert!(graph.contains(field), "shield graph missing {field}: {graph}");
    }
    assert!(
        !graph.contains("\"kind\":\"source\""),
        "Shield must project from its AST region, not an opaque source fallback: {graph}"
    );

    let before = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&before);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"replace_source\",\"revision\":\"{}\",\"source\":\"fn run() {{\\n    #Shield {{\\n        print(\\\"after\\\")\\n    }}\\n}}\\n\",\"source_edit\":true}}",
        revision
    );
    let out = jet::Canvas::apply_transaction_json(&path, &edit).expect("edit Shield source");
    assert!(out.contains("\"changed\":true"), "{out}");
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("#Shield {"), "{after}");
    assert!(after.contains("print(\"after\")"), "{after}");
    let graph = jet::Canvas::graph_json_for_file(&path).expect("shield graph after edit");
    assert!(graph.contains("\"kind\":\"shield\""), "{graph}");
    assert!(graph.contains("\"title\":\"\\\"after\\\"\""), "{graph}");
}

#[test]
fn canvas_projects_async_task_rail() {
    let path = write_fixture("task_rail", CANVAS_TASK_RAIL_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in ["\"async\"", "\"kind\":\"taskgroup\"", "\"title\":\".task\""] {
        assert!(
            graph.contains(field),
            "task rail graph missing {field}: {graph}"
        );
    }
}

#[test]
fn canvas_projects_event_dispatchers_from_core_event() {
    let path = write_fixture("event_dispatchers", CANVAS_EVENT_DISPATCHER_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"event_dispatchers\"",
        "\"kind\":\"event_stream_create\"",
        "\"kind\":\"event_subscribe\"",
        "\"kind\":\"event_subscribe_once\"",
        "\"kind\":\"event_subscribe_priority\"",
        "\"kind\":\"event_emit\"",
        "\"lifetime\":\"EventScope-owned\"",
        "\"debug_overlay\":\"EventTrace delivered/queued/dropped\"",
        "\"semantics\":\"core.event_source_truth\"",
    ] {
        assert!(
            graph.contains(field),
            "event dispatcher graph missing {field}: {graph}"
        );
    }
}

#[test]
fn canvas_projects_trait_impl_authoring_and_writes_impl_stub() {
    let path = write_fixture("trait_interface", CANVAS_TRAIT_INTERFACE_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    for field in [
        "\"interfaces\"",
        "\"kind\":\"trait_interface\"",
        "\"trait\":\"Drawable\"",
        "\"signature\":\"fn render(self) -> String\"",
        "\"create_trait_impl\"",
    ] {
        assert!(
            graph.contains(field),
            "trait graph missing {field}: {graph}"
        );
    }

    let src = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&src);
    let edit = format!(
        "{{\"schema_version\":1,\"op\":\"create_trait_impl\",\"revision\":\"{}\",\"type_name\":\"Badge\",\"trait_name\":\"Drawable\"}}",
        revision
    );
    let out = jet::Canvas::apply_transaction_json(&path, &edit).expect("create trait impl");
    assert!(out.contains("\"changed\":true"), "{out}");
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("impl Badge.Drawable"), "{after}");
    assert!(after.contains("fn render(self) -> String"), "{after}");
    assert!(after.contains("return \"canvas\""), "{after}");
    let graph = jet::Canvas::graph_json_for_file(&path).expect("trait impl graph");
    assert!(graph.contains("\"kind\":\"trait_impl\""), "{graph}");
    assert!(
        graph.contains("\"diagnostic_affordance\":\"surface_missing_trait_members\""),
        "{graph}"
    );
}

#[test]
fn canvas_projects_task_flow_authoring_facts() {
    let path = write_fixture("task_flow", CANVAS_TASK_FLOW_FIXTURE);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");

    for field in [
        "\"task_flows\"",
        "\"kind\":\"structured_task_scope\"",
        "\"kind\":\"channel_create\"",
        "\"kind\":\"taskgroup_spawn\"",
        "\"kind\":\"channel_send\"",
        "\"kind\":\"channel_receive\"",
        "\"kind\":\"taskgroup_join_all\"",
        "\"rail\":\"async\"",
        "\"semantics\":\"core.tasks_source_truth\"",
    ] {
        assert!(graph.contains(field), "task flow missing {field}: {graph}");
    }
}

#[test]
fn canvas_hardening_projection_suite_covers_blueprint_backlog_constructs() {
    let cases = [
        (
            "control",
            CANVAS_COVERAGE_FIXTURE,
            [
                "\"kind\":\"loop\"",
                "\"kind\":\"branch\"",
                "\"kind\":\"function\"",
                "\"kind\":\"assign\"",
                "\"layout_hints\":{\"algorithm\":\"source-order-v1\"",
            ],
        ),
        (
            "data",
            CANVAS_DATA_FIXTURE,
            [
                "\"kind\":\"expr\"",
                "\"kind\":\"variant\"",
                "\"title\":\"construct\"",
                "\"type\":\"Int\"",
                "\"source_span\"",
            ],
        ),
        (
            "rails",
            CANVAS_RAILS_FIXTURE,
            [
                "\"wire_kind\":\"fallible\"",
                "\"kind\":\"unsafe\"",
                "\"proof\"",
                "\"debug_overlay\":\"idle\"",
                "\"source\":\"front-end facts\"",
            ],
        ),
        (
            "task",
            CANVAS_TASK_RAIL_FIXTURE,
            [
                "\"async\"",
                "\"kind\":\"taskgroup\"",
                "\"title\":\".task\"",
                "\"pins\"",
                "\"wires\"",
            ],
        ),
    ];

    for (tag, src, required) in cases {
        let path = write_fixture(&format!("hardening_{tag}"), src);
        let first = jet::Canvas::graph_json_for_file(&path).expect("first projection");
        let second = jet::Canvas::graph_json_for_file(&path).expect("second projection");
        assert_eq!(first, second, "{tag} projection/layout drifted");
        assert!(
            count_occurrences(&first, "\"graph_id\":\"fn:") >= 1,
            "{tag} graph should expose at least one function graph: {first}"
        );
        assert!(
            count_occurrences(&first, "\"node_id\":\"fn:") >= 2,
            "{tag} graph should expose multiple source-backed nodes: {first}"
        );
        assert!(
            count_occurrences(&first, "\"source_span\":{\"start\":") >= 2,
            "{tag} graph should expose source spans for nodes/pins/inline facts: {first}"
        );
        assert!(
            !first.contains("\"kind\":\"source\""),
            "{tag} supported Canvas construct fell back to opaque source node: {first}"
        );
        for field in required {
            assert!(first.contains(field), "{tag} missing {field}: {first}");
        }
    }
}

#[test]
fn canvas_source_span_mapping_survives_source_edits_without_drift() {
    let path = write_fixture("span_drift", CANVAS_FIXTURE);
    let before = fs::read_to_string(&path).unwrap();
    let old_total = before.find("total := square").expect("old total span");
    let old_revision = jet::Canvas::source_revision(&before);
    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let summarize_graph_id = field_before(&graph, "\"title\":\"summarize\"", "graph_id");
    let insert = format!(
        "{{\"schema_version\":1,\"op\":\"insert_call\",\"revision\":\"{}\",\"graph_id\":\"{}\",\"callee\":\"print\",\"args\":[\"\\\"shift\\\"\"]}}",
        old_revision, summarize_graph_id
    );

    jet::Canvas::apply_transaction_json(&path, &insert).expect("insert before span drift check");
    let after = fs::read_to_string(&path).unwrap();
    let new_total = after.find("total := square").expect("new total span");
    assert_ne!(
        old_total, new_total,
        "insert should shift later source spans"
    );

    let new_revision = jet::Canvas::source_revision(&after);
    let query = format!(
        "{{\"schema_version\":1,\"op\":\"source_to_graph\",\"revision\":\"{}\",\"start\":{},\"end\":{}}}",
        new_revision,
        new_total,
        new_total + "total".len()
    );
    let mapped =
        jet::Canvas::query_json_for_file(&path, &query).expect("source-to-graph after edit");
    assert!(mapped.contains("\"op\":\"source_to_graph\""), "{mapped}");
    assert!(mapped.contains("\"title\":\"total\""), "{mapped}");
    assert!(mapped.contains(":binding"), "{mapped}");

    let stale = format!(
        "{{\"schema_version\":1,\"op\":\"source_to_graph\",\"revision\":\"{}\",\"start\":{},\"end\":{}}}",
        old_revision,
        old_total,
        old_total + "total".len()
    );
    let err = jet::Canvas::query_json_for_file(&path, &stale).unwrap_err();
    assert!(err.contains("\"kind\":\"conflict\""), "{err}");
}

#[test]
fn canvas_unsupported_and_invalid_actions_return_canvas_errors_without_rustc() {
    let path = write_fixture("unsupported_diagnostics", CANVAS_FIXTURE);
    let before = fs::read_to_string(&path).unwrap();
    let revision = jet::Canvas::source_revision(&before);

    let bad_schema = format!(
        "{{\"schema_version\":99,\"op\":\"noop\",\"revision\":\"{}\"}}",
        revision
    );
    let err = jet::Canvas::apply_transaction_json(&path, &bad_schema).unwrap_err();
    assert!(err.contains("\"protocol\":\"jet.canvas.edit\""), "{err}");
    assert!(err.contains("\"kind\":\"schema\""), "{err}");
    assert!(!err.contains("rustc"), "{err}");

    let bad_query = format!(
        "{{\"schema_version\":1,\"op\":\"teleport_node\",\"revision\":\"{}\"}}",
        revision
    );
    let err = jet::Canvas::query_json_for_file(&path, &bad_query).unwrap_err();
    assert!(err.contains("\"protocol\":\"jet.canvas.query\""), "{err}");
    assert!(err.contains("\"kind\":\"unsupported\""), "{err}");
    assert!(!err.contains("rustc"), "{err}");

    let graph = jet::Canvas::graph_json_for_file(&path).expect("canvas graph");
    let inline_id = field_before(&graph, "\"source\":\"total > 10\"", "inline_expr_id");
    let invalid_expr = format!(
        "{{\"schema_version\":1,\"op\":\"edit_inline_expr\",\"revision\":\"{}\",\"inline_expr_id\":\"{}\",\"new_expr\":\"missing_name\"}}",
        revision, inline_id
    );
    let err = jet::Canvas::apply_transaction_json(&path, &invalid_expr).unwrap_err();
    assert!(err.contains("\"protocol\":\"jet.canvas.edit\""), "{err}");
    assert!(err.contains("\"kind\":\"diagnostic\""), "{err}");
    assert!(err.contains("Error [E0107]"), "{err}");
    assert!(!err.contains("rustc"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn canvas_blueprint_parity_matrix_is_classified() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs/plans/epoch-6/canvas-blueprint-parity-matrix.md");
    let matrix = fs::read_to_string(&path).expect("Canvas parity matrix");
    let allowed = [
        "shipped",
        "claimed",
        "planned",
        "blocked-by-ballot",
        "rejected-as-Blueprint-semantic-debt",
        "not-yet-applicable",
    ];
    let required_areas = [
        "Workbench",
        "Hotkeys",
        "Node model",
        "Pins and wires",
        "Types",
        "Comments",
        "Functions",
        "Macros/collapse",
        "Events",
        "Debugger",
        "Search/refactor",
        "Source control",
        "Public protocol",
        "Extensibility",
        "Tests",
    ];

    for area in required_areas {
        assert!(
            matrix.contains(&format!("| {area} |")),
            "missing area {area}"
        );
    }

    let mut rows = 0;
    for line in matrix.lines().filter(|line| line.starts_with("| ")) {
        if line.starts_with("| Area |") || line.starts_with("|---|") {
            continue;
        }
        let cols: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(cols.len(), 5, "matrix row must have five columns: {line}");
        assert!(
            allowed.contains(&cols[3]),
            "unknown Canvas parity status `{}` in row: {line}",
            cols[3]
        );
        if cols[3] == "claimed" || cols[3] == "shipped" {
            assert!(
                ["interaction:", "protocol:", "projection:", "grep:"]
                    .iter()
                    .any(|prefix| cols[4].starts_with(prefix)),
                "Canvas matrix row with implementation proof must carry a ratchet class: {line}"
            );
        }
        if cols[3] == "shipped" {
            assert!(
                cols[4].starts_with("interaction:tests/canvas_scenarios.rs::"),
                "shipped Canvas matrix row must cite an interaction scenario: {line}"
            );
        }
        rows += 1;
    }

    assert!(
        rows >= 50,
        "Canvas parity matrix should cover the UE audit breadth"
    );
    for status in allowed {
        assert!(
            matrix.contains(&format!("- `{status}`")),
            "status vocabulary `{status}` must be documented"
        );
    }
}
