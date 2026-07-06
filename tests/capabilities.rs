//! c110 (P0): capability reporting is derived from semantic facts (resolved
//! Core calls, `#Unsafe` gates, FFI declarations), not from scanning generated
//! Rust text. These tests pin the new behavior and the bugs it fixes.
//!
//! Note: the legacy `Capabilities::from_rust` text scan turned out to be stale —
//! the codegen helper names it looks for (`jet_fs_`, …) have drifted, so it
//! silently under-reported real Core use. The sema-derived path does not depend
//! on lowered-Rust spelling at all, which is the point of the card.

use jet::Capabilities;

fn caps(src: &str) -> Capabilities {
    jet::compile(src).expect("program compiles").capabilities
}

/// A plain program declares no special capabilities.
#[test]
fn plain_program_has_no_capabilities() {
    let c = caps(r#"fn run() { print("hi"); }"#);
    assert!(
        !c.uses_network
            && !c.uses_file_io
            && !c.uses_unsafe
            && !c.uses_ffi
            && !c.uses_crypto
            && !c.uses_concurrency,
        "hello world should have no capabilities: {}",
        c.summary()
    );
}

/// A filesystem call sets `uses_file_io` from the resolved Core call.
#[test]
fn fs_call_sets_file_io() {
    let c = caps(
        r#"
use core.files as fs
fn run() { x :: fs.read("a") ?? ""; print(x); }
"#,
    );
    assert!(
        c.uses_file_io,
        "fs.read should set file_io; got {}",
        c.summary()
    );
    assert!(!c.uses_network, "fs.read must not set network");
}

/// A clock call sets `uses_concurrency` (time).
#[test]
fn time_call_sets_concurrency() {
    let c = caps(
        r#"
use core.time as time
fn run() { t :: time.now(); print("{t}"); }
"#,
    );
    assert!(
        c.uses_concurrency,
        "time.now should set concurrency; got {}",
        c.summary()
    );
}

/// The headline of c110: a program that merely *prints a string* containing an
/// old codegen marker must NOT be reported as using that capability. The
/// sema-derived path knows no network call is made; the legacy text scan is
/// fooled by the literal in the generated Rust — proving why c110 matters.
#[test]
fn capabilities_ignore_rust_text_lookalikes() {
    let out = jet::compile(r#"fn run() { print("jet_net_ is only text"); }"#).expect("compiles");
    assert!(
        !out.capabilities.uses_network,
        "sema must not flag network for a mere string literal"
    );
    assert!(
        Capabilities::from_rust(&out.rust).uses_network,
        "the legacy text scan false-positives on the literal (a bug c110 removes)"
    );
}
