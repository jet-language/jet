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
    assert!(json.contains("\"kind\":\"call\""));
    assert!(json.contains("\"kind\":\"variable_get\""));
    assert!(json.contains("\"kind\":\"constant\""));
    assert!(json.contains("\"type\":\"Int\""));
    assert!(json.contains("\"wire_kind\":\"data\""));
    assert!(json.contains("\"wire_kind\":\"control\""));
    assert!(json.contains("\"type\":\"exec\""));
    assert!(json.contains("\"capability\":\"control\""));
    assert!(json.contains("\"inline_exprs\""));
    assert!(json.contains("total > 10"));
    let square_call = first_node_id_containing(&json, ":call:square");
    let binding = first_node_id_containing(&json, ":stmt:1:binding");
    assert!(
        json.contains(&format!(
            "\"from_pin\":\"{square_call}:output:then\",\"to_pin\":\"{binding}:input:exec\",\"wire_kind\":\"control\""
        )),
        "execution rail should run data-producing calls before dependent bindings: {json}"
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
        "\"kind\":\"dispatch\"",
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
fn canvas_stale_transaction_conflicts_without_writing() {
    let path = write_fixture("stale", CANVAS_FIXTURE);
    let stale = "{\"schema_version\":1,\"op\":\"rename_binding\",\"revision\":\"sha256-stale\",\"from\":\"total\",\"to\":\"score\"}";
    let err = jet::Canvas::apply_transaction_json(&path, stale).unwrap_err();
    assert!(err.contains("\"kind\":\"conflict\""), "{err}");
    assert!(fs::read_to_string(&path).unwrap().contains("total"));
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
        "\"authority\":[\"canvas.source_edit:current_file\"]",
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
        "\"authority\":[\"canvas.source_edit:current_file\"]",
        "\"package_id\":\"local-source\"",
        "\"touched_files\":[\"current_file\"]",
        "+    square(1)",
    ] {
        assert!(out.contains(field), "action preview missing {field}: {out}");
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        src,
        "Canvas action preview must not write source"
    );

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
        graph.contains("\"kind\":\"call\",\"title\":\"area\""),
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
fn canvas_protocol_doc_matches_v1_graph_and_edit_shape() {
    let doc_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/reference/canvas-protocol.md");
    let doc = fs::read_to_string(doc_path).expect("Canvas protocol reference");
    for term in [
        "jet.canvas.graph",
        "jet.canvas.edit",
        "schema_version",
        "source_id",
        "revision",
        "fmt_fingerprint",
        "graphs",
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
        "jet.canvas.query",
        "source_to_graph",
        "preview_rename",
        "actions",
        "palette_entries",
        "jet.canvas.action",
        "preview_canvas_action",
        "canvas.action",
        "checked-tir+jit",
        "source_transaction_only",
        "external adapter",
        "jet.canvas.source_control",
        "dirty",
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
        "\"kind\":\"dispatch\"",
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
                "\"kind\":\"dispatch\"",
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
        .join("tools/Tower/docs/plans/epoch-6/canvas-blueprint-parity-matrix.md");
    let matrix = fs::read_to_string(&path).expect("Canvas parity matrix");
    let allowed = [
        "shipped",
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
        if cols[3] == "shipped" {
            assert!(
                cols[4].contains("tests/") || cols[4].contains("#275"),
                "shipped Canvas matrix row must name its test ratchet: {line}"
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
