//! Keep docs/08-stdlib.md in sync with docs/stdlib.md (M14 API freeze).

use std::fs;
use std::path::PathBuf;

const HEADER: &str = "<!-- AUTO-GENERATED from docs/stdlib.md — do not edit by hand.\n\
     Regenerate: ./scripts/gen_stdlib.sh or UPDATE_DOCS=1 cargo test gen_stdlib_doc -->\n\n";

#[test]
fn gen_stdlib_doc() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("docs/stdlib.md");
    let dest = root.join("docs/08-stdlib.md");
    let body = fs::read_to_string(&source).expect("docs/stdlib.md");
    let expected = format!("{HEADER}{body}");

    if std::env::var("UPDATE_DOCS").is_ok() {
        fs::write(&dest, &expected).unwrap();
        eprintln!("wrote {}", dest.display());
    } else if dest.is_file() {
        let on_disk = fs::read_to_string(&dest).unwrap();
        assert_eq!(
            on_disk, expected,
            "docs/08-stdlib.md is stale — run: UPDATE_DOCS=1 cargo test gen_stdlib_doc"
        );
    } else {
        panic!("docs/08-stdlib.md missing — run: UPDATE_DOCS=1 cargo test gen_stdlib_doc");
    }
}
