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
    assert!(html.contains("answer()"));
    assert!(html.contains("run.jet#L"));
    assert!(markdown.contains("## Doctests"));
    assert!(markdown.contains("1 + 1 // => 2"));
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
    let snapshot = include_str!("ui_lint/undocumented_public_api.warn");
    for line in snapshot.lines().filter(|line| !line.is_empty()) {
        assert!(rendered.contains(line), "missing `{line}` in:\n{rendered}");
    }
}
