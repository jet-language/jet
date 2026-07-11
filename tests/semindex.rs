//! D-SEMINDEX1 integration tests for the stable semantic-index API.

use jet_semindex::{open, SCHEMA_VERSION};
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
    assert_eq!(SCHEMA_VERSION, 3);
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
