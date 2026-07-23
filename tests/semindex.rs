//! D-SEMINDEX1 integration tests for the stable semantic-index API.

use jet_semindex::{
    open, open_symbols, SemanticProvenance, SemanticSymbol, SemanticSymbolIndex,
    SemanticSymbolKind, SymbolKind, ViewProjectionFact, ViewSourceFact, SCHEMA_VERSION,
};
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features")
        .join(name)
}

fn temp_fixture(name: &str, src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_semindex_{}", std::process::id()));
    fs::create_dir_all(&dir).expect("semindex temp dir");
    let path = dir.join(name);
    fs::write(&path, src).expect("write semindex fixture");
    path
}

fn fact(
    identity: &str,
    name: &str,
    qualified_name: &str,
    kind: SemanticSymbolKind,
    provenance: SemanticProvenance,
) -> SemanticSymbol {
    SemanticSymbol {
        identity: identity.to_string(),
        name: name.to_string(),
        qualified_name: qualified_name.to_string(),
        owner: qualified_name.split_once('.').map(|(owner, _)| owner.to_string()),
        module_path: "test".to_string(),
        kind,
        signature: identity.to_string(),
        summary: String::new(),
        examples: Vec::new(),
        provenance,
        span: None,
        lexical_scope: None,
    }
}

#[test]
fn semantic_visibility_prefers_live_and_local_bindings() {
    let index = SemanticSymbolIndex::new(vec![
        fact(
            "builtin:keyword:answer", "answer", "answer", SemanticSymbolKind::Keyword,
            SemanticProvenance::Builtin { module: "syntax".to_string() },
        ),
        fact(
            "import:test::api.answer::answer", "answer", "answer",
            SemanticSymbolKind::Function, SemanticProvenance::Session,
        ),
        fact(
            "fn:session::answer", "answer", "answer", SemanticSymbolKind::Function,
            SemanticProvenance::Session,
        ),
        fact(
            "session:binding:answer", "answer", "answer", SemanticSymbolKind::Local,
            SemanticProvenance::Session,
        ),
    ]);
    assert_eq!(
        index.resolve_visible("answer").expect("visible answer").identity,
        "session:binding:answer"
    );
    let completions = index.complete_visible("ans", None);
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].identity, "session:binding:answer");
}

#[test]
fn contextual_result_names_are_not_advertised_as_keywords() {
    let path = temp_fixture("contextual_result_names.jet", "fn run() {}\n");
    let symbols = open_symbols(&path).expect("semantic symbols");
    for name in ["Ok", "Err"] {
        assert!(
            symbols.symbols().iter().all(|symbol| {
                symbol.identity != format!("builtin:keyword:{name}")
                    && !(symbol.name == name && symbol.kind == SemanticSymbolKind::Keyword)
            }),
            "{name} must stay a contextual identifier, not a keyword"
        );
    }
}

#[test]
fn numeric_destination_conversions_and_parse_are_cataloged() {
    let symbols = SemanticSymbolIndex::language();
    assert!(symbols.lookup_qualified("Int.parse").is_some());
    assert!(symbols.lookup_qualified("Float.parse").is_some());
    let narrow = symbols
        .lookup_qualified("F32.from_float")
        .expect("F32 narrowing catalog entry");
    assert!(
        narrow.signature.ends_with("-> F32 ? String"),
        "{}",
        narrow.signature
    );
    assert!(symbols.lookup_qualified("U8.from_u64").is_some());
    assert!(symbols.lookup_qualified("Float.from_i8").is_some());
    assert!(symbols.lookup_qualified("I64.from_f32").is_some());
    assert!(symbols.lookup_qualified("F64.from_u32").is_some());
}

#[test]
fn source_distinct_and_unit_conversion_members_are_cataloged() {
    let src = r#"
#Numeric UserId :: distinct Int
#UnitFamily(Currency) { usd }

fn run() {}
"#;
    let path = temp_fixture(
        "source_numeric_members.jet",
        src,
    );
    let symbols = open_symbols(&path).expect("source semantic symbols");

    let user = symbols
        .lookup_qualified("UserId.from_u8")
        .expect("source distinct conversion member");
    assert_eq!(user.owner.as_deref(), Some("UserId"));
    assert_eq!(user.signature, "UserId.from_u8(value: U8) -> UserId");

    let unit = symbols
        .lookup_qualified("Usd.from_int")
        .expect("unit-family conversion member");
    assert_eq!(unit.owner.as_deref(), Some("Usd"));

    let user_offset = src.find("#Numeric UserId").unwrap();
    assert!(symbols
        .complete_visible_at(
            "from_u",
            Some("UserId"),
            jet_semindex::SemanticVisibilityAnchor {
                module_path: path.to_string_lossy().as_ref(),
                offset: Some(user_offset),
                session_top_level: false,
            },
        )
        .iter()
        .any(|symbol| symbol.qualified_name == "UserId.from_u8"));
}

#[test]
fn affine_unit_point_and_delta_members_are_cataloged() {
    let src = r#"
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}

fn run() {}
"#;
    let path = temp_fixture("source_affine_unit_members.jet", src);
    let symbols = open_symbols(&path).expect("source semantic symbols");

    assert!(symbols.lookup_qualified("CelsiusPoint.from_float").is_some());
    assert!(symbols.lookup_qualified("CelsiusDelta.from_float").is_some());
    assert!(symbols.lookup_qualified("Celsius.from_float").is_none());
}

#[test]
fn semantic_visibility_orders_items_imports_and_builtins() {
    let builtin = fact(
        "builtin:keyword:answer", "answer", "answer", SemanticSymbolKind::Keyword,
        SemanticProvenance::Builtin { module: "syntax".to_string() },
    );
    let import = fact(
        "import:test::api.answer::answer", "answer", "answer",
        SemanticSymbolKind::Function, SemanticProvenance::Session,
    );
    let item = fact(
        "fn:session::answer", "answer", "answer", SemanticSymbolKind::Function,
        SemanticProvenance::Session,
    );
    assert_eq!(
        SemanticSymbolIndex::new(vec![builtin.clone(), import.clone()])
            .resolve_visible("answer").unwrap().identity,
        import.identity
    );
    assert_eq!(
        SemanticSymbolIndex::new(vec![builtin, import, item.clone()])
            .resolve_visible("answer").unwrap().identity,
        item.identity
    );
}

#[test]
fn semantic_visibility_uses_current_module_context() {
    let mut current = fact(
        "fn:current::answer", "answer", "answer", SemanticSymbolKind::Function,
        SemanticProvenance::Source { module_path: "current.jet".to_string() },
    );
    current.module_path = "current.jet".to_string();
    let mut foreign_local = fact(
        "local:other::answer", "answer", "answer", SemanticSymbolKind::Local,
        SemanticProvenance::Source { module_path: "other.jet".to_string() },
    );
    foreign_local.module_path = "other.jet".to_string();
    let index = SemanticSymbolIndex::new(vec![current.clone(), foreign_local]);
    assert_eq!(
        index.resolve_visible_in("answer", Some("current.jet")).unwrap().identity,
        current.identity
    );
    assert_eq!(index.complete_visible_in("ans", None, Some("current.jet")).len(), 1);
}

#[test]
fn semantic_visibility_retains_explicit_qualified_alternatives() {
    let index = SemanticSymbolIndex::new(vec![
        fact(
            "method:List.answer", "answer", "List.answer", SemanticSymbolKind::Member,
            SemanticProvenance::Builtin { module: "core".to_string() },
        ),
        fact(
            "method:Map.answer", "answer", "Map.answer", SemanticSymbolKind::Member,
            SemanticProvenance::Builtin { module: "core".to_string() },
        ),
    ]);
    assert_eq!(index.resolve_visible("List.answer").unwrap().identity, "method:List.answer");
    let qualified = index.complete_visible("", Some("List"));
    assert_eq!(qualified.len(), 1);
    assert_eq!(qualified[0].identity, "method:List.answer");
    assert!(index.lookup_identity("method:Map.answer").is_some());
}

#[test]
fn semindex_schema_version() {
    // D-EFFECT-OMIT1 added effect provenance and normalized inferred rows.
    assert_eq!(SCHEMA_VERSION, 11);
}

#[test]
fn semindex_reconstructs_checked_output_callable() {
    let path = fixture("tooling/output_callable.jet");
    let index = open(&path).expect("Output example indexes");
    let output = index.outputs().first().expect("resolved Output fact");
    assert_eq!(output.binding, "app");
    assert_eq!(output.kind, "Executable");
    assert_eq!(output.name, "checked-output");
    assert_eq!(output.entry.name, "launch");
    assert!(output.entry.identity.ends_with("::launch"));
    assert_eq!(output.entry.authority, "safe-jet");
    assert_ne!(output.entry.definition_span, output.entry.reference_span);
    let json = index.to_json();
    for field in [
        "\"outputs\":[{",
        "\"entry\":{\"identity\"",
        "\"authority\":\"safe-jet\"",
        "\"effects\":[\"Io\"]",
    ] {
        assert!(json.contains(field), "semindex JSON missing {field}: {json}");
    }
}

#[test]
fn jet_inspect_semindex_reports_checked_output() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "inspect",
            "semindex",
            fixture("tooling/output_callable.jet").to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("jet inspect semindex");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"outputs\":[{\"binding\":\"app\""), "{json}");
    assert!(json.contains("\"identity\":\"output_callable::launch\""), "{json}");
}

#[test]
fn semantic_symbols_carry_shared_docs_and_provenance() {
    let src = r#"
/// Scores one name.
/// Example: score("Ada")
fn score(name: String) -> Int {
    return 1
}

fn run() {
    local_total :: score("Ada")
    print(local_total)
}
"#;
    let path = temp_fixture("semantic_docs.jet", src);
    let symbols = open_symbols(&path).expect("semantic symbols");
    let score = symbols
        .lookup_identity(&format!("fn:module:{}::score", path.display()))
        .expect("score identity");
    assert_eq!(score.signature, "fn score(name: String) --[]-> Int");
    assert_eq!(score.summary, "Scores one name.");
    assert_eq!(score.examples, vec!["score(\"Ada\")"]);
    assert!(matches!(score.provenance, SemanticProvenance::Source { .. }));

    let local = symbols.lookup("local_total");
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].kind, SemanticSymbolKind::Local);
}

#[test]
fn semantic_symbols_keep_module_and_member_collisions_distinct() {
    let src = r#"
module first {
    struct Item {
        value: Int
    }
}

module second {
    struct Item {
        value: String
    }
}

fn run() {}
"#;
    let path = temp_fixture("semantic_collisions.jet", src);
    let symbols = open_symbols(&path).expect("semantic symbols");
    let items = symbols.lookup("Item");
    assert_eq!(items.len(), 2, "same spelling must retain module identity");
    assert_ne!(items[0].identity, items[1].identity);
    let values = symbols.lookup_member("Item", "value");
    assert_eq!(values.len(), 2, "same member spelling must retain owner identity");
    assert_ne!(values[0].identity, values[1].identity);
}

#[test]
fn semantic_symbols_include_language_builtins() {
    let path = temp_fixture("semantic_builtins.jet", "fn run() {}\n");
    let symbols = open_symbols(&path).expect("semantic symbols");
    let filter = symbols.lookup_qualified("List.filter").expect("List.filter");
    assert_eq!(filter.signature, "List.filter(f: fn(T) -> Bool) -> List<T>");
    assert_eq!(filter.summary, "Keeps items where f(item) is true.");
    assert!(matches!(filter.provenance, SemanticProvenance::Builtin { .. }));
    assert!(symbols.complete("fil", Some("List")).iter().any(|s| s.name == "filter"));
}

#[test]
fn semantic_symbols_include_module_and_selected_imports() {
    let root = std::env::temp_dir().join(format!("jet_semantic_imports_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("library.jet"),
        "pub fn score(name: String) -> Int { return 1 }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"./library\" as api\nuse api.{score as imported_score}\nfn run() { print(imported_score(\"Ada\")) }\n",
    )
    .unwrap();
    let symbols = open_symbols(&main).expect("import symbols");
    assert_eq!(symbols.lookup("api").len(), 1);
    let imported = symbols.lookup("imported_score");
    assert_eq!(imported.len(), 1);
    assert!(imported[0].identity.starts_with("import:"));
    assert!(imported[0]
        .signature
        .contains("score(name: String) --[]-> Int"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn semindex_hello_json_shape() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    let json = idx.to_json();
    assert!(json.starts_with('{'));
    assert!(json.contains("\"schema_version\":11"));
    assert!(json.contains("\"definition_facts\""));
    assert!(json.contains("\"definitions\""));
    assert!(json.contains("\"instances\""));
    assert!(json.contains("\"identity\""));
    assert!(json.contains("\"scope_identity\""));
    assert!(json.contains("\"members\""));
    assert!(json.contains("\"run\""));
}

#[test]
fn semindex_identity_stable_across_reorder() {
    let a = r#"
module math {
    pub fn double(n: Int) -> Int {
        return n * 2
    }
}

struct Point {
    x: Int
    y: Int

    fn sum(self) -> Int {
        return self.x + self.y
    }
}

impl Point {
    fn origin() -> Point {
        return Point.{x: 0, y: 0}
    }
}

enum Light {
    Red
    Green
}

fn helper(p: Point) -> Int {
    return p.sum()
}

fn run() {
    p :: Point.origin()
    print(helper(p))
}
"#;
    let b = r#"
enum Light {
    Red
    Green
}

fn helper(p: Point) -> Int {
    return p.sum()
}

struct Point {
    x: Int
    y: Int

    fn sum(self) -> Int {
        return self.x + self.y
    }
}

module math {
    pub fn double(n: Int) -> Int {
        return n * 2
    }
}

impl Point {
    fn origin() -> Point {
        return Point.{x: 0, y: 0}
    }
}

fn run() {
    p :: Point.origin()
    print(helper(p))
}
"#;
    let path = temp_fixture("identity_reorder.jet", a);
    let before = open(&path).expect("first fixture indexes");
    fs::write(&path, b).expect("rewrite semindex fixture");
    let after = open(&path).expect("reordered fixture indexes");

    let mut before_ids: Vec<String> = before
        .definitions()
        .iter()
        .map(|d| d.identity.clone())
        .collect();
    let mut after_ids: Vec<String> = after
        .definitions()
        .iter()
        .map(|d| d.identity.clone())
        .collect();
    before_ids.sort();
    after_ids.sort();
    assert_eq!(before_ids, after_ids);

    assert!(before_ids.iter().any(|id| id.contains("module:")));
    assert!(before_ids.iter().any(|id| id.contains("fn:")));
    assert!(before_ids
        .iter()
        .any(|id| id.contains("type:") && id.contains("Point")));
    assert!(before_ids
        .iter()
        .any(|id| id.contains("field:") && id.contains("Point.x")));
    assert!(before_ids
        .iter()
        .any(|id| id.contains("variant:") && id.contains("Light.Red")));
    assert!(before_ids
        .iter()
        .any(|id| id.contains("method:") && id.contains("Point.sum")));
}

#[test]
fn returned_view_provenance_is_structured_and_changes_signature_id() {
    let path = temp_fixture(
        "view_provenance.jet",
        "fn pick(left: [Int], right: [Int]) -> View<Int> { return left[0..1] }\nfn run() {}\n",
    );
    let left = open(&path).expect("parameter-0 view provenance indexes");
    let pick = left.lookup("pick").expect("pick definition");
    assert!(matches!(pick.kind, SymbolKind::Function { .. }));
    assert_eq!(pick.view_provenance.len(), 1);
    let provenance = &pick.view_provenance[0];
    assert!(provenance.output_path.is_empty());
    assert_eq!(provenance.source, ViewSourceFact::Parameter(0));
    assert_eq!(provenance.projections, vec![ViewProjectionFact::Range]);
    assert!(!provenance.mutable);
    let left_signature = left
        .definition_facts()
        .iter()
        .find(|fact| fact.name == "pick")
        .unwrap()
        .signature_id
        .clone();
    let json = left.to_json();
    assert!(json.contains("\"view_provenance\":[{\"output_path\":[],\"source\":{\"kind\":\"parameter\",\"index\":0}"));
    assert!(json.contains("\"projections\":[{\"kind\":\"range\"}]"));

    fs::write(
        &path,
        "fn pick(left: [Int], right: [Int]) -> View<Int> { return right[0..1] }\nfn run() {}\n",
    )
    .unwrap();
    let right = open(&path).expect("parameter-1 view provenance indexes");
    let right_signature = &right
        .definition_facts()
        .iter()
        .find(|fact| fact.name == "pick")
        .unwrap()
        .signature_id;
    assert_ne!(&left_signature, right_signature);
}

#[test]
fn aggregate_view_provenance_preserves_slots_and_changes_signature_id() {
    let source = |right_owner: &str| format!(r#"
struct Pair {{ left: View<Int>, right: View<Int> }}

fn pair(left: [Int], right: [Int]) -> Pair {{
    left_view :: left[0..1]
    right_view :: {right_owner}[0..1]
    return Pair.{{ left: left_view, right: right_view }}
}}

fn run() {{}}
"#);
    let path = temp_fixture("aggregate_view_provenance.jet", &source("right"));
    let distinct = open(&path).expect("aggregate view provenance indexes");
    let pair = distinct.lookup("pair").expect("pair definition");
    assert_eq!(pair.view_provenance.len(), 2);
    assert_eq!(pair.view_provenance[0].output_path, vec!["left"]);
    assert_eq!(pair.view_provenance[0].source, ViewSourceFact::Parameter(0));
    assert_eq!(pair.view_provenance[1].output_path, vec!["right"]);
    assert_eq!(pair.view_provenance[1].source, ViewSourceFact::Parameter(1));
    let distinct_signature = distinct
        .definition_facts()
        .iter()
        .find(|fact| fact.name == "pair")
        .unwrap()
        .signature_id
        .clone();
    let json = distinct.to_json();
    assert!(json.contains("\"output_path\":[\"left\"],\"source\":{\"kind\":\"parameter\",\"index\":0}"));
    assert!(json.contains("\"output_path\":[\"right\"],\"source\":{\"kind\":\"parameter\",\"index\":1}"));

    fs::write(&path, source("left")).unwrap();
    let changed = open(&path).expect("changed aggregate view provenance indexes");
    let changed_signature = &changed
        .definition_facts()
        .iter()
        .find(|fact| fact.name == "pair")
        .unwrap()
        .signature_id;
    assert_ne!(&distinct_signature, changed_signature);
}

#[test]
fn semindex_structural_audit_reports_signature_change() {
    let a = r#"
struct Point {
    x: Int
}

fn run() {}
"#;
    let b = r#"
struct Point {
    x: Float
}

fn run() {}
"#;
    let path = temp_fixture("audit_change.jet", a);
    let before = open(&path).expect("first fixture indexes");
    fs::write(&path, b).expect("rewrite semindex fixture");
    let after = open(&path).expect("changed fixture indexes");

    let audit = before.structural_audit(&after);
    assert!(audit.added.is_empty());
    assert!(audit.removed.is_empty());
    assert!(audit.changed.iter().any(|id| id.contains("Point")));
}

#[test]
fn semindex_structural_audit_tracks_callable_signature_changes() {
    let a = r#"
fn score(name: String) -> Int {
    return 1
}

fn run() {}
"#;
    let b = r#"
fn score(name: String, bonus: Int) -> Float {
    return 1.0
}

fn run() {}
"#;
    let path = temp_fixture("audit_callable_change.jet", a);
    let before = open(&path).expect("first fixture indexes");
    fs::write(&path, b).expect("rewrite semindex fixture");
    let after = open(&path).expect("changed fixture indexes");

    let audit = before.structural_audit(&after);
    assert!(
        !audit
            .added
            .iter()
            .any(|id| id.starts_with("fn:") && id.ends_with("::score")),
        "added: {:?}",
        audit.added
    );
    assert!(
        !audit
            .removed
            .iter()
            .any(|id| id.starts_with("fn:") && id.ends_with("::score")),
        "removed: {:?}",
        audit.removed
    );
    assert!(audit
        .changed
        .iter()
        .any(|id| id.starts_with("fn:") && id.ends_with("::score")));
}

#[test]
fn semindex_unified_loop_slots_and_state_scope_are_structural() {
    let src = r#"fn run() {
    loop item; [1, 2, 3]; 2 {
        print(item)
    }
    loop cursor := 0; cursor < 1; cursor += 1 {
        print(cursor)
    }
}
"#;
    let path = temp_fixture("unified_loop_slots.jet", src);
    let index = open(&path).expect("unified loops index");
    let slots = index
        .structural_nodes()
        .iter()
        .map(|node| node.slot.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for slot in ["source", "stride", "init", "condition", "afterthought", "body"] {
        assert!(slots.contains(slot), "missing loop structural slot `{slot}`: {slots:?}");
    }

    let inner_def = index
        .definitions()
        .iter()
        .filter(|def| def.name == "cursor")
        .max_by_key(|def| def.def_span.start)
        .expect("inner state definition");
    for reference in index.references_to("cursor") {
        assert_eq!(reference.target.as_ref().unwrap().def_span, inner_def.def_span);
    }

    let out_of_scope = jet::check_document(
        "state_scope.jet",
        "fn run() {\n    loop cursor := 0; cursor < 1; cursor += 1 {}\n    print(cursor)\n}\n",
    );
    assert!(
        out_of_scope.iter().any(|diagnostic| {
            diagnostic.code == "E0107" && diagnostic.what.contains("cursor")
        }),
        "state binding must end with the loop: {out_of_scope:?}"
    );
}

#[test]
fn definition_ancestry_identity_ignores_signature_and_body_shape() {
    let path = temp_fixture(
        "ancestry_identity.jet",
        "fn score(n: Int) -> Int { return n + 1 }\nfn run() { print(score(1)) }\n",
    );
    let before = open(&path).expect("first fixture indexes");
    fs::write(
        &path,
        "fn score(n: Float, bonus: Float) -> Float { return n * bonus }\nfn run() { print(score(1.0, 2.0)) }\n",
    )
    .unwrap();
    let after = open(&path).expect("changed fixture indexes");
    let before_score = before
        .definition_facts()
        .iter()
        .find(|f| f.name == "score")
        .unwrap();
    let after_score = after
        .definition_facts()
        .iter()
        .find(|f| f.name == "score")
        .unwrap();
    assert_eq!(before_score.stable_id, after_score.stable_id);
    assert_ne!(before_score.signature_id, after_score.signature_id);
    assert_ne!(before_score.content_id, after_score.content_id);
}

#[test]
fn semindex_effects_and_calls() {
    let idx = open(&fixture("effects/effects.jet")).expect("effects indexes");
    assert!(idx.lookup("report").is_some());
    assert!(!idx.call_edges().is_empty());
    let report_effects = idx.effect_of("report").expect("report has effects");
    assert!(!report_effects.inferred.is_empty() || !report_effects.direct.is_empty());
}

#[test]
fn semindex_projects_omitted_inferred_effects() {
    let path = temp_fixture(
        "inferred_effect_signature.jet",
        "fn announce() { print(\"hello\") }\nfn run() { announce() }\n",
    );
    let index = open(&path).expect("inferred effects index");
    let announce = index.effect_of("announce").expect("announce effects");
    assert_eq!(announce.inferred, vec!["Io"]);
    assert!(announce.callees.is_empty(), "direct effect has no call edge");
    assert_eq!(announce.provenance.len(), 1);
    assert_eq!(announce.provenance[0].effect, "Io");
    assert_eq!(announce.provenance[0].call_path, vec!["announce"]);
    assert_eq!(announce.provenance[0].spans.len(), 1);
    let run = index.effect_of("run").expect("run effects");
    assert_eq!(
        run.provenance[0].call_path,
        vec!["run", "inferred_effect_signature::announce"]
    );
    assert_eq!(run.provenance[0].spans.len(), 2);
}

#[test]
fn semindex_projects_inline_module_inferred_effects() {
    let path = temp_fixture(
        "inline_inferred_effect_signature.jet",
        "module inner { pub fn announce() { print(\"hello\") } }\nfn run() { inner.announce() }\n",
    );
    let symbols = open_symbols(&path).expect("inline inferred effects index");
    let announce = symbols.lookup("announce");
    assert_eq!(announce.len(), 1);
    assert!(announce[0].signature.contains("--[Io]->"), "{}", announce[0].signature);
}

#[test]
fn semindex_effect_provenance_covers_open_and_trait_dispatch() {
    let path = temp_fixture(
        "effect_provenance_origins.jet",
        "trait Shape { fn area(self) --[Io]-> Int; }\nfn dynamic(shape: Shape) -> Int { return shape.area(); }\nfn apply(f: fn() -> Int) -> Int { return f(); }\nfn stored(f: ^fn() -> Int) -> Int { g :: f; return g(); }\nfn run() {}\n",
    );
    let index = open(&path).expect("effect provenance index");

    let apply = index.effect_of("apply").expect("open callback effects");
    assert!(!apply.maximal);
    assert!(apply.inferred.is_empty());
    assert!(apply.provenance.is_empty());

    let stored = index.effect_of("stored").expect("stored callback effects");
    assert!(stored.maximal);
    assert_eq!(stored.provenance.len(), stored.inferred.len());
    assert!(stored.provenance.iter().all(|origin| !origin.spans.is_empty()));

    let dynamic = index.effect_of("dynamic").expect("trait dispatch effects");
    let io = dynamic
        .provenance
        .iter()
        .find(|origin| origin.effect == "Io")
        .expect("Io provenance");
    assert_eq!(io.call_path.len(), 2);
    assert_eq!(io.spans.len(), 2);
}

#[test]
fn semindex_via_contracts_have_provenance_without_invocation() {
    let path = temp_fixture(
        "effect_via_provenance.jet",
        "fn bounded(act: fn() --[Io]->) --[via act]-> {}\nfn open(act: fn()) --[via act]-> {}\nfn run() {}\n",
    );
    let index = open(&path).expect("via provenance index");

    let bounded = index.effect_of("bounded").expect("bounded via effects");
    assert_eq!(bounded.inferred, vec!["Io"]);
    assert_eq!(bounded.provenance.len(), 1);
    assert_eq!(bounded.provenance[0].spans.len(), 1);

    let open = index.effect_of("open").expect("unbounded via effects");
    assert!(open.maximal);
    assert_eq!(open.provenance.len(), open.inferred.len());
    assert!(open.provenance.iter().all(|origin| !origin.spans.is_empty()));
}

#[test]
fn semindex_preserves_via_effect_row_in_signatures() {
    let path = temp_fixture(
        "effect_via.jet",
        "fn invoke(act: fn() --[Io]->) --[via act]-> { act() }\nfn run() {}\n",
    );
    let symbols = open_symbols(&path).expect("via function indexes");
    let invoke = symbols
        .lookup("invoke")
        .into_iter()
        .next()
        .expect("invoke symbol");
    assert_eq!(
        invoke.signature,
        "fn invoke(act: fn() --[Io]->) --[via act]->"
    );
}

#[test]
fn semindex_references() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    assert!(!idx.references_to("print").is_empty());
}

#[test]
fn semindex_indexes_loop_label_definition_and_dot_exit_references() {
    let path = temp_fixture(
        "loop_label_refs.jet",
        "fn run() {\n    outer :: loop {\n        if true { outer.next() }\n        outer.break()\n    }\n}\n",
    );
    let idx = open(&path).expect("loop label fixture indexes");
    let outer = idx.lookup("outer").expect("loop label definition");
    assert!(matches!(
        outer.kind,
        SymbolKind::Local {
            mutable: false,
            ty: None
        }
    ));
    assert_eq!(idx.references_to("outer").len(), 2);
}

#[test]
fn semindex_dossier_stitches_scattered_members() {
    let src = r#"
trait DrawThing {
    fn render(self) -> String
}

struct Widget {
    title: String

    fn label(self) -> String {
        return self.title
    }
}

impl Widget {
    fn size(self) -> Int {
        return 1
    }
}

impl Widget.DrawThing {
    fn render(self) -> String {
        return self.label()
    }
}

fn run() {
    w :: Widget.{title: "ok"}
    print(w.render())
}
"#;
    let path = temp_fixture("dossier_scattered.jet", src);
    let idx = open(&path).expect("dossier fixture indexes");
    let dossier = idx.dossier("Widget");
    let signatures: Vec<String> = dossier
        .members
        .iter()
        .map(|m| format!("{}:{}", m.name, m.signature))
        .collect();
    assert!(signatures.iter().any(|s| s == "title:title: String"));
    assert!(signatures
        .iter()
        .any(|s| s.contains("label:fn label() -> String")));
    assert!(signatures
        .iter()
        .any(|s| s.contains("size:fn size() -> Int")));
    assert!(signatures
        .iter()
        .any(|s| s.contains("render:fn render() -> String")));
    let json = dossier.to_json();
    assert!(json.contains("\"target\":\"Widget\""));
    assert!(json.contains("\"trait_impl\""));
    assert!(json.contains("\"inherent_impl\""));
}

#[test]
fn semindex_dossier_bypass_facts() {
    // D-LINTPOLICY1=A (the override law, card #505): every spelled bypass
    // — `#Unsafe(reason)` region, `#Unsafe(reason) fn`, `.drop(reason)`, and
    // `#[allow(lint)]` — surfaces as a fact in the dossier, program-wide.
    let src = r#"
struct Invoice {
    #[allow(float_money)]
    price: Float,
}

#Unsafe("caller must ensure the pointer is valid") fn risky_fn() {
    print("danger")
}

fn run() {
    #Unsafe("index checked against len") {
        print("audited")
    }
    #Unsafe("risky_fn's contract is upheld here") {
        risky_fn()
    }
    fetch_it().drop("telemetry only")
}

fn fetch_it() -> Int {
    return 1
}
"#;
    let path = temp_fixture("dossier_bypass_facts.jet", src);
    let idx = open(&path).expect("bypass-fact fixture indexes");
    let dossier = idx.dossier("run");

    let kinds: Vec<&str> = dossier
        .bypass_facts
        .iter()
        .map(|b| b.kind.as_str())
        .collect();
    assert!(
        kinds.contains(&"lint_allow"),
        "expected a lint_allow bypass fact, got: {kinds:?}"
    );
    assert!(
        kinds.contains(&"unsafe_region"),
        "expected an unsafe_region bypass fact, got: {kinds:?}"
    );
    assert!(
        kinds.contains(&"unsafe_fn"),
        "expected an unsafe_fn bypass fact, got: {kinds:?}"
    );
    assert!(
        kinds.contains(&"explicit_drop"),
        "expected an explicit_drop bypass fact, got: {kinds:?}"
    );

    let allow = dossier
        .bypass_facts
        .iter()
        .find(|b| b.kind.as_str() == "lint_allow")
        .expect("lint_allow fact present");
    assert_eq!(allow.site, "price");
    assert_eq!(allow.detail, "float_money");

    let drop = dossier
        .bypass_facts
        .iter()
        .find(|b| b.kind.as_str() == "explicit_drop")
        .expect("explicit_drop fact present");
    assert_eq!(drop.detail, "telemetry only");

    let json = dossier.to_json();
    assert!(json.contains("\"bypass_facts\""));
    assert!(json.contains("\"lint_allow\""));

    let text = dossier.render_text();
    assert!(text.contains("bypass facts"));
    assert!(text.contains("float_money"));
}

#[test]
fn jet_semindex_cli_json_smoke() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = fixture("basics/hello.jet");
    let out = std::process::Command::new(bin)
        .args(["inspect", "semindex", path.to_str().unwrap(), "--json"])
        .output()
        .expect("jet inspect semindex");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"schema_version\":11"));
}

#[test]
fn jet_dossier_cli_json_smoke() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = fixture("types/traits.jet");
    let out = std::process::Command::new(bin)
        .args(["inspect", "dossier", path.to_str().unwrap(), "Square", "--json"])
        .output()
        .expect("jet inspect dossier");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"target\":\"Square\""));
    assert!(text.contains("\"members\""));
}

#[test]
fn shape6_inspect_routes_and_retired_bare_snapshots() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let help = std::process::Command::new(&bin).arg("help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for group_name in ["inspect", "registry"] {
        let group = jet::CLI::command_group(group_name).unwrap();
        for action in group.actions {
            let command = format!("jet {} {}", group.name, action.name);
            assert!(help.contains(&command), "jet help omitted {command}");
        }
    }

    for verb in ["build", "run", "test", "fmt"] {
        assert!(jet::CLI::is_canonical_top_level(verb), "jet {verb} must stay flat");
        assert!(jet::CLI::moved_command(verb).is_none(), "jet {verb} must not redirect");
    }

    for (verb, handler_text) in [
        ("dossier", "`jet inspect dossier` needs an entry file"),
        ("schema", "`jet inspect schema` needs a verb"),
        ("expand", "`jet inspect expand` needs an entry file"),
        ("live", "jet inspect live needs a process id"),
        ("semindex", "`jet inspect semindex` needs an entry file"),
    ] {
        let grouped = std::process::Command::new(&bin)
            .args(["inspect", verb])
            .output()
            .unwrap();
        assert!(!grouped.status.success(), "jet inspect {verb} needs test input");
        let stderr = String::from_utf8(grouped.stderr).unwrap();
        assert!(stderr.contains(handler_text), "jet inspect {verb}: {stderr}");

        let bare = std::process::Command::new(&bin)
            .args([verb, "sentinel"])
            .output()
            .unwrap();
        assert_eq!(bare.status.code(), Some(2), "bare jet {verb}");
        assert_eq!(
            String::from_utf8(bare.stderr).unwrap(),
            format!(
                "Error [E2101]: `{verb}` moved under `jet inspect`.\n Why: infrequent commands live in a named area so daily Jet commands stay easy to scan.\n Fix: run `jet inspect {verb} sentinel`.\n"
            )
        );
    }
}
