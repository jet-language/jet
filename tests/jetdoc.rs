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
        "\"failure_contract\":\"Result<Int, Err>\"",
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
    assert!(markdown.contains("failure: Result<Int, Err> (implicit default !Err)"));
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
