//! D-ONCE-LAW1 guard for Core semantics shared with comptime.

use std::fs;
use std::path::Path;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn mime_and_email_have_one_semantic_home() {
    let codegen = read("crates/jet-codegen/src/Codegen/mod.rs");
    let core_calls = read("crates/jet-comptime/src/Comptime/Methods/core_calls.rs");
    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let url_mime = read("crates/jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");

    let kernel = "JetStd/Mime.rs";
    assert!(codegen.contains(kernel), "AOT must embed {kernel}");
    assert!(core_calls.contains(kernel), "comptime/interpreter must include {kernel}");
    assert!(
        codegen.contains("CoreLib/Email.rs"),
        "AOT must embed email kernel"
    );
    let jit = read("crates/jet-jit/src/Net.rs");
    assert!(
        read("crates/jet-comptime/src/Comptime/EmailAdapter.rs").contains("CoreLib/Email.rs")
    );
    assert!(jit.contains("CoreLib/Email.rs"));
    assert!(jit.contains("CoreLib/JetStd/UrlMime.rs"));

    assert!(
        !pure.contains("fn email_"),
        "comptime must not regain an email implementation"
    );
    assert!(!pure.contains("\"html\" | \"htm\" => Some(\"text/html\")"));
    assert!(url_mime.starts_with("    include!(\"Mime.rs\");"));
    assert!(!url_mime.contains("fn jet_mime_token"));
}
