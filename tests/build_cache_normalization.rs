//! D-BUILDNORM1=A (Tower #85): the content-addressed build-cache key is
//! `SHA256(canonical_bytes(pre-sema AST) + profile + toolchain-salt)`. These
//! tests pin the six normalization properties of that key — the contract a
//! description alone can't enforce (I4/I5).
//!
//! The key is computed exactly as the compiler computes it for a real build:
//! load the program through `jet::Loader::load_entry_with_overlay` (lex + parse
//! + import resolution, *no sema*) and hash it with
//! `jet::CanonicalAST::ast_cache_key`.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `src` to a fresh temp dir under a *fixed* basename (so the module's
/// filename-derived alias is identical across variants — only the AST content
/// varies), load its pre-sema bundle, and return the cache key.
fn key_with(src: &str, profile_tag: &str, version: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir: PathBuf =
        std::env::temp_dir().join(format!("jet-buildnorm-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.jet");
    std::fs::write(&file, src).unwrap();
    let bundle = jet::Loader::load_entry_with_overlay(file.to_str().unwrap(), None, false)
        .unwrap_or_else(|d| panic!("test program should parse:\n{src}\n{:?}", d));
    let key = jet::CanonicalAST::ast_cache_key(&bundle, profile_tag, version);
    std::fs::remove_dir_all(&dir).ok();
    key
}

/// The common case: default profile, a fixed version salt.
fn key(src: &str) -> String {
    key_with(src, "default", "test-version")
}

#[test]
fn whitespace_insensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn  add( a : Int ,  b : Int )  {\n\n        print(  a  +  b  )\n\n}\n";
    assert_eq!(key(a), key(b), "reformatting must not change the key");
}

#[test]
fn comment_insensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "/// doc comment\n// leading comment\nfn add(a: Int, b: Int) {\n  // inline\n  print(a + b) // trailing\n}\n";
    assert_eq!(key(a), key(b), "adding comments must not change the key");
}

#[test]
fn rename_sensitive() {
    // The D-BUILDNORM1 ratified example: renaming locals changes the key.
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn add(x: Int, y: Int) {\n    print(x + y)\n}\n";
    assert_ne!(key(a), key(b), "renaming a parameter must change the key");
}

#[test]
fn reorder_sensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn add(a: Int, b: Int) {\n    print(b + a)\n}\n";
    assert_ne!(key(a), key(b), "reordering operands must change the key");
}

#[test]
fn profile_sensitive() {
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let default = key_with(src, "default", "v");
    let small = key_with(src, "small", "v");
    let release = key_with(src, "release", "v");
    assert_ne!(default, small, "profiles must not share a cache entry");
    assert_ne!(default, release);
    assert_ne!(small, release);
}

#[test]
fn version_sensitive() {
    // A toolchain (or manifest) salt change must invalidate the entry — the
    // guard that a codegen change never serves a stale binary for an identical
    // AST (Tower #85 §1 step 3).
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    assert_ne!(
        key_with(src, "default", "0.1.0"),
        key_with(src, "default", "0.2.0"),
        "a version-salt change must change the key"
    );
}

// ── Extra discrimination guards (belt-and-suspenders for the serializer) ──

#[test]
fn operator_sensitive() {
    let a = "fn f(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn f(a: Int, b: Int) {\n    print(a - b)\n}\n";
    assert_ne!(key(a), key(b), "a different operator must change the key");
}

#[test]
fn literal_sensitive() {
    let a = "fn f() {\n    print(1)\n}\n";
    let b = "fn f() {\n    print(2)\n}\n";
    assert_ne!(key(a), key(b), "a different literal must change the key");
}

#[test]
fn string_literal_content_sensitive() {
    // String-literal contents are part of the program and must change the key.
    // (The exact `Span {…}`-lookalike-inside-a-literal robustness of the span
    // stripper is pinned by the CanonicalAST unit test; Jet reads `{ }` in
    // strings as interpolation, so we use brace-free literals here.)
    let a = "fn f() {\n    print(\"Span start 1 end 2\")\n}\n";
    let b = "fn f() {\n    print(\"Span start 7 end 3\")\n}\n";
    assert_ne!(
        key(a),
        key(b),
        "string-literal contents must survive span-stripping"
    );
}

#[test]
fn stable_across_reloads() {
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    assert_eq!(key(src), key(src), "same input must yield the same key");
    assert_eq!(key(src).len(), 64, "key is a 64-hex SHA-256 digest");
}
