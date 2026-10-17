//! Read-text ABI boundaries: borrowing consumers and genuinely owned sinks.
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc};

#[test]
fn read_text_views_forward_return_and_callables() {
    assert!(have_rustc(), "copy/borrow proof requires rustc");
    let source = r#"
fn length(text: String) -> Int { text.len() }
fn forward(text: String) -> Int { length(text) }
fn owned(text: String) -> String { text.copy() }
fn apply(callback: fn(String) -> Int, text: String) -> Int { callback(text) }
fn inspect<T>(_value: T) -> Int { 7 }
fn generic_forward(text: String) -> Int { inspect(text) }
fn run() {
    text := "  hello  "
    view :: text.trim()
    print(forward(view))
    print(owned(view))
    print(apply(length, view))
    reader :: (value: String) -> (length(value) + text.len())
    print(apply(reader, view))
    print(generic_forward(view))
    print(inspect(text))
    print(inspect("literal"))
}
"#;
    let (code, output) = build_and_run("copy_borrow_text_boundaries", source);
    assert_eq!(code, 0);
    assert_eq!(output, "5\nhello\n5\n14\n7\n7\n7\n");
}

#[test]
fn heterogeneous_text_queries_keep_owned_collection_elements() {
    assert!(have_rustc(), "copy/borrow proof requires rustc");
    let source = r#"
fn query(values: [String], mapping: [String:Int], text: String) {
    print(values.contains(text))
    print(values.index_of(text) ?? -1)
    print(values.count(text))
    print(mapping.has_key(text))
    print(mapping.get(text) ?? -1)
}
fn run() {
    source := " a "
    view :: source.trim()
    values :: ["a", "b", "a"]
    mapping :: [String:Int]{"a": 3}
    query(values, mapping, view)
}
"#;
    let (code, output) = build_and_run("copy_borrow_text_queries", source);
    assert_eq!(code, 0);
    assert_eq!(output, "true\n0\n2\ntrue\n3\n");
}

#[test]
fn optional_text_auth_projection_borrows_payload() {
    assert!(have_rustc(), "copy/borrow proof requires rustc");
    let source = r#"
use core.auth
fn verify(issuer: String) {
    token :: #Credential "invalid"
    key :: [U8]{0, 1, 2}
    _claims :: auth.verify_jwt(token, key: key, audience: "gateway", issuer: issuer, clock_skew: 0ms) ?? {
        print(false)
        return
    }
    print(true)
}
fn run() { verify("partner") }
"#;
    let (code, output) = build_and_run("copy_borrow_optional_auth", source);
    assert_eq!(code, 0);
    assert_eq!(output, "false\n");
}
