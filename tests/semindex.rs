//! D-SEMINDEX1 integration tests for the stable semantic-index API.

use jet_semindex::{
    open, open_symbols, SemanticProvenance, SemanticSymbol, SemanticSymbolIndex,
    SemanticSymbolKind, SCHEMA_VERSION,
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
    assert_eq!(SCHEMA_VERSION, 3);
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
    assert!(json.contains("\"schema_version\":3"));
    assert!(json.contains("\"definitions\""));
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
    assert!(text.contains("\"schema_version\":3"));
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
