//! Snapshot tests for every user-facing diagnostic (invariant I4).
//!
//! Each tests/ui/NAME.jet has a sibling NAME.stderr holding the exact
//! rendered output. To update after an INTENTIONAL wording change:
//!
//!     UPDATE_EXPECT=1 cargo test
//!
//! Never bless a snapshot you haven't read against docs/04-diagnostics.md.
//! These files are the product: the error messages ARE the language's UX.

use std::fs;
use std::path::PathBuf;

#[test]
fn ui_snapshots() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let ext = jet::syntax::FILE_EXT;
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();

    let mut checked = 0;
    for path in entries {
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Companion programs that apply the Fix: from a sibling ui test.
        if name.contains(".fixed.") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        // Stable path string so snapshots match on every machine.
        let shown_path = format!("tests/ui/{}", name);

        let actual = match jet::compile(&src) {
            Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
            Ok(_) => "(no errors)\n".to_string(),
        };

        let expect_path = path.with_extension("stderr");
        if std::env::var("UPDATE_EXPECT").is_ok() {
            fs::write(&expect_path, &actual).unwrap();
        } else {
            let expected = fs::read_to_string(&expect_path).unwrap_or_default();
            assert_eq!(
                actual, expected,
                "\nui snapshot mismatch for {}\n(if the new output is intentional and matches docs/04-diagnostics.md, run: UPDATE_EXPECT=1 cargo test)\n",
                name
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 7,
        "expected the ui suite to contain tests, found {}",
        checked
    );
}
