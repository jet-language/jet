//! D-SEMINDEX1 integration tests for the stable semantic-index API.

use jet_semindex::{
    open, open_symbols, SemanticProvenance, SemanticSymbolKind, SymbolKind, ViewProjectionFact,
    ViewSourceFact, SCHEMA_VERSION,
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

#[test]
fn semindex_schema_version() {
    // Bumped 4 -> 5 by commit d80e2cba: SemIndex gained a top-level
    // `instances` fact array (generic-module instantiation identity —
    // fingerprint + full key, D-GENMOD-IDENTITY1) alongside the E0859
    // fingerprint-collision guard in Sema/Bundle.rs.
    assert_eq!(SCHEMA_VERSION, 9);
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
    assert_eq!(score.signature, "fn score(name: String) -> Int");
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
    assert!(imported[0].signature.contains("score(name: String) -> Int"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn semindex_hello_json_shape() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    let json = idx.to_json();
    assert!(json.starts_with('{'));
    assert!(json.contains("\"schema_version\":9"));
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
fn semindex_references() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    assert!(!idx.references_to("print").is_empty());
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
    // — `@Unsafe(reason)` region, `@Unsafe(reason) fn`, `.drop(reason)`, and
    // `#[allow(lint)]` — surfaces as a fact in the dossier, program-wide.
    let src = r#"
struct Invoice {
    #[allow(float_money)]
    price: Float,
}

@Unsafe("caller must ensure the pointer is valid") fn risky_fn() {
    print("danger")
}

fn run() {
    @Unsafe("index checked against len") {
        print("audited")
    }
    @Unsafe("risky_fn's contract is upheld here") {
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
    assert!(text.contains("\"schema_version\":9"));
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
