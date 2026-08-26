//! Focused jetdoc fixture tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
        let path = root.join(format!(
            "jetdoc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("jetdoc scratch directory");
        Self(path)
    }

    fn copy_fixture(&self, name: &str) -> PathBuf {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jetdoc")
            .join(name);
        let destination = self.0.join(name);
        fs::copy(source, &destination).expect("copy jetdoc fixture");
        destination
    }

    fn copy_fixture_package(&self, entry: &str) -> PathBuf {
        self.copy_fixture("package.jet");
        self.copy_fixture(entry)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn jet_doc(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .expect("jet doc command")
}

#[test]
fn json_and_local_outputs_are_stable_and_complete() {
    let scratch = Scratch::new();
    let entry = scratch.copy_fixture("run.jet");
    let entry = entry.to_string_lossy().into_owned();
    let first = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let second = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(first.stdout, second.stdout, "jetdoc JSON order drifted");
    let json = String::from_utf8(first.stdout).expect("UTF-8 jetdoc JSON");
    for needle in [
        "\"schema_version\":1",
        "\"modules\"",
        "\"kind\":\"struct\"",
        "\"kind\":\"function\"",
        "\"kind\":\"marker\"",
        "\"kind\":\"protocol\"",
        "\"impls\"",
        "\"kind\":\"trait_impl\"",
        "\"doctests\"",
        "answer()",
        "\"failure_contract\":\"Int !\"",
        "\"failure_source\":\"implicit default !Err\"",
        "\"examples\":[\"`answer()`\"]",
        "\"expression\":\"1 + 1\",\"expected\":\"2\"",
        "\"link\":\"run.jet#L35\"",
        "run.jet#L",
    ] {
        assert!(json.contains(needle), "jetdoc JSON lacks {needle}: {json}");
    }
    let order = [
        "\"qualified_name\":\"Checked\"",
        "\"qualified_name\":\"Choice\"",
        "\"qualified_name\":\"Displayable\"",
        "\"qualified_name\":\"Displayable::display\"",
        "\"qualified_name\":\"Payment\"",
        "\"qualified_name\":\"Widget\"",
        "\"qualified_name\":\"answer\"",
    ];
    let positions = order
        .iter()
        .map(|needle| json.find(needle).expect("ordered jetdoc item"))
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

    let local = jet_doc(&["doc", &entry], &scratch.0);
    assert!(local.status.success(), "{}", String::from_utf8_lossy(&local.stderr));
    let html = fs::read_to_string(scratch.0.join("docs/index.html")).expect("HTML output");
    let markdown = fs::read_to_string(scratch.0.join("docs/index.md")).expect("Markdown output");
    assert!(html.contains("Jet Documentation"));
    assert!(html.contains("<h4>Examples</h4>"));
    assert!(html.contains("<li><code>`answer()`</code></li>"));
    assert!(html.contains("1 + 1 // =&gt; 2"));
    assert!(html.contains("../run.jet#L35"));
    assert!(html.contains("run.jet#L"));
    assert!(markdown.contains("Examples:\n\n- `answer()`"));
    assert!(markdown.contains("failure: Int ! (implicit default !Err)"));
    assert!(markdown.contains("## Doctests"));
    assert!(markdown.contains("1 + 1 // => 2"));
    assert!(markdown.contains("[Source](../run.jet#L35)"));
    assert!(markdown.contains("run.jet#L"));
}

#[test]
fn undocumented_public_api_has_a_command_snapshot() {
    let scratch = Scratch::new();
    let entry = scratch.copy_fixture("undocumented.jet");
    let entry = entry.to_string_lossy().into_owned();
    let output = jet_doc(&["doc", "--check", &entry], &scratch.0);
    assert!(!output.status.success());
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = rendered.replace(&entry, "tests/fixtures/jetdoc/undocumented.jet");
    let snapshot = include_str!("ui_lint/undocumented_public_api.warn");
    assert_eq!(rendered, snapshot, "undocumented-public-api UI snapshot drifted");
}

#[test]
fn fixture_outputs_match_stable_goldens() {
    let scratch = Scratch::new();
    let entry = scratch.copy_fixture("undocumented.jet");
    let entry = entry.to_string_lossy().into_owned();

    let json = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(json.status.success(), "{}", String::from_utf8_lossy(&json.stderr));
    assert_eq!(
        json.stdout,
        include_bytes!("fixtures/jetdoc/undocumented.json").as_slice()
    );

    let local = jet_doc(&["doc", &entry], &scratch.0);
    assert!(local.status.success(), "{}", String::from_utf8_lossy(&local.stderr));
    assert_eq!(
        fs::read(scratch.0.join("docs/index.md")).expect("Markdown output"),
        include_bytes!("fixtures/jetdoc/undocumented.md").as_slice()
    );
    assert_eq!(
        fs::read(scratch.0.join("docs/index.html")).expect("HTML output"),
        include_bytes!("fixtures/jetdoc/undocumented.html").as_slice()
    );
}

#[test]
fn package_outputs_match_stable_goldens() {
    let scratch = Scratch::new();
    let entry = scratch.copy_fixture_package("undocumented.jet");
    let entry = entry.to_string_lossy().into_owned();

    let json = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(json.status.success(), "{}", String::from_utf8_lossy(&json.stderr));
    assert_eq!(
        json.stdout,
        include_bytes!("fixtures/jetdoc/package-undocumented.json").as_slice(),
        "package JSON output drifted"
    );

    let local = jet_doc(&["doc", &entry], &scratch.0);
    assert!(local.status.success(), "{}", String::from_utf8_lossy(&local.stderr));
    assert_eq!(
        fs::read(scratch.0.join("docs/index.md")).expect("Markdown output"),
        include_bytes!("fixtures/jetdoc/package-undocumented.md").as_slice(),
        "package Markdown output drifted"
    );
    assert_eq!(
        fs::read(scratch.0.join("docs/index.html")).expect("HTML output"),
        include_bytes!("fixtures/jetdoc/package-undocumented.html").as_slice(),
        "package HTML output drifted"
    );
}

#[test]
fn package_fixture_rejects_order_and_content_drift() {
    let scratch = Scratch::new();
    let entry = scratch.copy_fixture_package("ordered.jet");
    let entry = entry.to_string_lossy().into_owned();

    let first = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let second = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(first.stdout, second.stdout, "package JSON order is not stable");

    let json = String::from_utf8(first.stdout).expect("UTF-8 package docs JSON");
    for needle in [
        "\"summary\":\"Alpha API.\"",
        "\"signature\":\"pub fn alpha() Int -> 6\\nfailure: Int ! (implicit default !Err)\"",
        "\"summary\":\"Zulu API.\"",
        "\"signature\":\"pub fn zulu() Int -> 7\\nfailure: Int ! (implicit default !Err)\"",
        "\"link\":\"ordered.jet#L7\"",
        "\"link\":\"ordered.jet#L4\"",
    ] {
        assert!(json.contains(needle), "package docs lack {needle}: {json}");
    }
    let order = [
        "\"qualified_name\":\"alpha\"",
        "\"qualified_name\":\"run\"",
        "\"qualified_name\":\"zulu\"",
    ];
    let positions = order
        .iter()
        .map(|needle| json.find(needle).expect("ordered package item"))
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn nested_state_docs_stay_on_the_struct_item() {
    let scratch = Scratch::new();
    let entry = scratch.0.join("nested_state.jet");
    fs::write(
        &entry,
        "pub struct Door { state { Closed, Open } }\nfn run() {}\n",
    )
    .expect("write nested state fixture");
    let entry = entry.to_string_lossy().into_owned();
    let output = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).expect("UTF-8 jetdoc JSON");
    assert!(json.contains("\"kind\":\"struct\""));
    assert!(json.contains("\"signature\":\"struct Door { state { Closed, Open } }\""));
    assert!(!json.contains("\"kind\":\"state\""));
}

#[test]
fn checked_text_contract_is_rendered_in_generated_docs() {
    let scratch = Scratch::new();
    let entry = scratch.0.join("checked_text.jet");
    fs::write(
        &entry,
        r#"#PubFile
#Error
enum PatternError { Bad }

/// A checked pattern.
Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = PatternError

    fn check(text: String) !PatternError -[]> {
        return
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return ""
    }
}

fn run() {}
"#,
    )
    .expect("write checked-text docs fixture");
    let entry = entry.to_string_lossy().into_owned();

    let json = jet_doc(&["doc", "--json", &entry], &scratch.0);
    assert!(
        json.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&json.stdout),
        String::from_utf8_lossy(&json.stderr)
    );
    let json = String::from_utf8(json.stdout).expect("UTF-8 docs JSON");
    assert!(json.contains("Pattern :: distinct String"), "{json}");
    assert!(json.contains("implements CheckedText"));
    assert!(json.contains("type Error = PatternError"));
    assert!(json.contains("encode_hole"));

    let rendered = jet_doc(&["doc", &entry], &scratch.0);
    assert!(rendered.status.success(), "{}", String::from_utf8_lossy(&rendered.stderr));
    let markdown = fs::read_to_string(scratch.0.join("docs/index.md")).expect("Markdown output");
    let html = fs::read_to_string(scratch.0.join("docs/index.html")).expect("HTML output");
    for output in [&markdown, &html] {
        assert!(output.contains("Pattern :: distinct String"), "{output}");
        assert!(output.contains("implements CheckedText"), "{output}");
        assert!(output.contains("type Error = PatternError"), "{output}");
        assert!(output.contains("encode_hole"), "{output}");
    }
}
