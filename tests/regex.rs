//! D-REGEXENGINE1 — core.regex is std-only in the generated prelude. Regex-only
//! programs must not ask for the hidden FFI bridge or the old `regex` crate.

use std::fs;

use std::process::Command;

mod common;
use common::have_rustc;

/// Compile, FFI-link, and run a regex program; return stdout.
fn run_regex(src: &str) -> String {
    // Unique dir per call so concurrent regex tests never clobber one another's
    // `regex_test.{jet,rs,_bin}` (the process id alone is shared across tests in
    // the same binary).
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jet_regex_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("regex_test.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();

    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected regex fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    assert!(
        out.ffi.is_none(),
        "core.regex must not produce an FFI bridge"
    );
    assert!(
        !out.rust.contains("regex::") && !out.rust.contains("extern crate regex"),
        "core.regex must not reference the old regex crate"
    );
    let user_rust = common::strip_scheduler_native(&common::strip_vetted_module(&out.rust, "jet_atomic_windows"));
    assert!(
        !user_rust.contains("unsafe"),
        "I1: regex output must not contain unsafe"
    );

    let rs = dir.join("regex_test.rs");
    let bin = dir.join("regex_test_bin");
    fs::write(&rs, &out.rust).unwrap();
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .status()
        .unwrap();
    assert!(status.success(), "I2: rustc rejected regex output");

    let run = Command::new(&bin).output().unwrap();
    assert!(
        run.status.success(),
        "regex program failed at runtime:\nstderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn have_toolchain() -> bool {
    have_rustc() && Command::new("cargo").arg("--version").output().is_ok()
}

#[test]
fn match_groups_find_replace_split() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping jet.regex integration test");
        return;
    }
    let src = r##"
use core.regex as re

fn run() {
    text :: "2024-06-21 build ok"

    // is_match. NB: `{N}` regex quantifiers are written `{{N}}` in Jet source —
    // single braces are string interpolation (S8).
    print(re.is_match("\\d{{4}}", text) ?? panic("p"))

    // match + capture groups: whole + each group
    m :: re.match("(\\d{{4}})-(\\d{{2}})-(\\d{{2}})", text) ?? panic("p")
    if m == Val(mat) {
        print(mat.group(0) ?? "x")
        print(mat.group(1) ?? "x")
        print(mat.group(2) ?? "x")
        print(mat.group(3) ?? "x")
        // out-of-range group is none
        print(mat.group(9) ?? "none")
    }

    // no match -> None optional
    none_match :: re.match("zzz", text) ?? panic("p")
    if none_match == None {
        print("no-match")
    }

    // find / find_all
    first :: re.find("\\d+", text) ?? panic("p")
    print(first ?? "none")
    nums :: re.find_all("\\d+", text) ?? panic("p")
    print(nums.len())

    // replace / replace_all (with group reference)
    print(re.replace("ok", text, "done") ?? panic("p"))
    print(re.replace_all("\\d", text, "#") ?? panic("p"))

    // split
    parts :: re.split("-", "a-b-c") ?? panic("p")
    print(parts.len())
}
"##;
    let out = run_regex(src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "true", "is_match");
    assert_eq!(lines[1], "2024-06-21", "group(0) whole match");
    assert_eq!(lines[2], "2024", "group(1)");
    assert_eq!(lines[3], "06", "group(2)");
    assert_eq!(lines[4], "21", "group(3)");
    assert_eq!(lines[5], "none", "out-of-range group is none");
    assert_eq!(lines[6], "no-match", "non-matching pattern yields None");
    assert_eq!(lines[7], "2024", "find first match");
    assert_eq!(lines[8], "3", "find_all count (2024, 06, 21)");
    assert_eq!(lines[9], "2024-06-21 build done", "replace first");
    assert_eq!(lines[10], "####-##-## build ok", "replace_all digits");
    assert_eq!(lines[11], "3", "split into 3 parts");
}

#[test]
fn bad_pattern_is_a_recoverable_error_not_a_crash() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping jet.regex error test");
        return;
    }
    // An unbalanced paren is a bad pattern: it must surface as the Result `Err`
    // branch at the boundary (linear-time crate, no panic), which `??` turns into
    // our own message — never a rustc/runtime crash (I2).
    let src = r##"
use core.regex as re

fn run() {
    if re.is_match("(unclosed", "abc") == {
        ok(_) -> { print("unexpected-ok") }
        err(e) -> { print("caught") }
    }
}
"##;
    let out = run_regex(src);
    assert_eq!(out.trim(), "caught", "bad pattern surfaces as Err");
}

#[test]
fn compiled_regex_flags_spans_names_limit_and_callback() {
    if !have_toolchain() {
        eprintln!("note: cargo/rustc not found; skipping core.regex API test");
        return;
    }
    let src = r##"
use core.regex as re

fn run() {
    flags :: re.flags(true, true, false)
    rx :: re.compile_with("^(?<word>[a-z]+)", flags) ?? panic("bad pattern")
    text :: "Ada\nlovelace"
    matches :: rx.matches(text)
    print(matches.len())
    first :: rx.match(text)
    if first == Val(mat) {
        print(mat.name("word") ?? "none")
        print(mat.start())
        print(mat.end())
        print(mat.group_start(1) ?? -1)
        print(mat.group_end(1) ?? -1)
    }

    sep :: re.compile(",\\s*") ?? panic("bad sep")
    pieces :: sep.split_limit("a, b, c", 2)
    print(pieces.len())
    print(pieces[1])

    word :: re.compile("\\w+") ?? panic("bad word")
    print(word.replace_all_with("a b", (m: Match) => "hit"))
}
"##;
    let out = run_regex(src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "2", "multiline + case-insensitive flags");
    assert_eq!(lines[1], "Ada", "named group");
    assert_eq!(lines[2], "0", "match start");
    assert_eq!(lines[3], "3", "match end");
    assert_eq!(lines[4], "0", "group start");
    assert_eq!(lines[5], "3", "group end");
    assert_eq!(lines[6], "2", "split limit len");
    assert_eq!(lines[7], "b, c", "split limit remainder");
    assert_eq!(lines[8], "hit hit", "callback replacement");
}
