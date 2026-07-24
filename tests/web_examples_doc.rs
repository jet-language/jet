//! Card #705: every shipped web example is indexed in docs and has proof output
//! or an explicit harness-only suffix in `examples/features/expected/web/`.

use std::fs;
use std::path::Path;

#[test]
fn web_examples_are_documented_and_have_expected_outputs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let web_dir = repo.join("examples/features/web");
    let expected_dir = repo.join("examples/features/expected/web");
    let doc = fs::read_to_string(repo.join("docs/sidequests/web-backend-wasm.md"))
        .expect("web backend doc");

    let mut jets: Vec<_> = fs::read_dir(&web_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jet"))
        .collect();
    jets.sort();

    assert!(!jets.is_empty(), "expected at least one web example");

    for jet in jets {
        let name = jet.file_name().unwrap().to_str().unwrap();
        assert!(
            doc.contains(name),
            "{name} missing from docs/sidequests/web-backend-wasm.md"
        );

        let stem = jet.file_stem().unwrap().to_str().unwrap();
        let native = expected_dir.join(format!("{stem}.out"));
        let web = expected_dir.join(format!("{stem}.web.out"));
        let harness = expected_dir.join(format!("{stem}.harness.out"));
        assert!(
            native.is_file() || web.is_file() || harness.is_file(),
            "{name} has no golden under examples/features/expected/web/"
        );
    }
}
