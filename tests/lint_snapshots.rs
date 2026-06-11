//! Snapshot tests for ownership lints (L02xx). Lints compile; warnings are
//! the product copy, pinned like errors in tests/ui.rs.

use std::fs;
use std::path::PathBuf;

#[test]
fn lint_snapshots() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui_lint");
    let ext = jet::syntax::FILE_EXT;
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    entries.sort();

    let mut checked = 0;
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let shown_path = format!("tests/ui_lint/{}", name);

        let out = jet::compile(&src).unwrap_or_else(|diags| {
            panic!(
                "lint fixture {} must compile:\n{}",
                name,
                jet::render_diagnostics(&shown_path, &src, &diags)
            );
        });
        assert!(
            !out.lints.is_empty(),
            "lint fixture {} should emit at least one lint",
            name
        );

        let actual = jet::render_diagnostics(&shown_path, &src, &out.lints);
        let expect_path = path.with_extension("warn");
        if std::env::var("UPDATE_EXPECT").is_ok() {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            assert_eq!(
                actual, expected,
                "\nlint snapshot mismatch for {}\n(run: UPDATE_EXPECT=1 cargo test lint_snapshots)\n",
                name
            );
        }
        checked += 1;
    }
    assert!(checked >= 2, "expected lint fixtures, found {}", checked);
}
