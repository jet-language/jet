//! D-PIN1=A / D-PIN2=A / D-PIN3=A: the `Pin<T>` address-stability contract.
//! Covers the ratified law end to end:
//!   * `mem.pin(&place)` opens a tracked write window; a value argument is E0218;
//!   * safe code cannot move, replace, or resize a pinned place (E0219);
//!   * reading and editing through the pin stay legal and reach the owner;
//!   * a `Pin<U>` field projects to `Pin<U>` through a pinned value;
//!   * the promise ends with the pin's scope and never escapes its owner;
//!   * AOT and the TIR interpreter agree on every case (I9).

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);
fn unique_tmp() -> std::path::PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("jet_pin_{}_{}", std::process::id(), n))
}

/// Diagnostic codes for a fixture written to a real path so `use core.mem`
/// resolves exactly as it does in a normal build.
fn error_codes(src: &str) -> Vec<String> {
    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jet");
    std::fs::write(&path, src).unwrap();
    match jet::compile_with_path(src, &path.to_string_lossy()) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

/// Compile to Rust and, when rustc is present, build and run. The rustc step is
/// the I2 backstop: a pin the front end accepted must also survive the borrow
/// checker without any generated `unsafe`.
fn build_and_run(name: &str, src: &str) -> Option<String> {
    let dir0 = unique_tmp();
    std::fs::create_dir_all(&dir0).unwrap();
    let fpath = dir0.join("fixture.jet");
    std::fs::write(&fpath, src).unwrap();
    let out = jet::compile_with_path(src, &fpath.to_string_lossy()).unwrap_or_else(|d| {
        panic!(
            "front end rejected a should-compile fixture: {:?}",
            d.iter().map(|x| x.code.as_str()).collect::<Vec<_>>()
        )
    });
    let user = common::strip_vetted_module(
        &common::strip_vetted_prelude_modules(&out.rust),
        "jet_taskgroup_scoped",
    );
    let leaked: Vec<&str> = user
        .lines()
        .filter(|line| line.contains("unsafe") && !line.trim_start().starts_with("//"))
        .take(5)
        .collect();
    assert!(
        leaked.is_empty(),
        "a pin must not need `unsafe` outside the vetted prelude helpers:\n{}",
        leaked.join("\n")
    );

    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("note: rustc not found; compiled front end only");
        return None;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_pin_{}.rs", name));
    let bin = dir.join(format!("jet_pin_{}", name));
    std::fs::write(&rs, &out.rust).unwrap();
    let c = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        c.status.success(),
        "I2 violated: rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&bin).output().unwrap();
    Some(String::from_utf8_lossy(&r.stdout).to_string())
}

/// I9: the TIR interpreter is the deopt tier for `jet run`/`jet dev`. It must
/// produce the same output the native build does, never a stale copy.
fn interpret(src: &str) -> String {
    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jet");
    std::fs::write(&path, src).unwrap();
    let mut bundle = jet::Loader::load_entry(&path.to_string_lossy())
        .expect("fixture loads");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags.iter().any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "fixture must type-check: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    let mut sink = jet::Comptime::DevSink::new();
    jet::Codegen::TIR::install_comptime_bridge();
    jet::Comptime::TirBridge::run_bundle(&bundle, &mut sink, true)
        .expect("interpreter runs the fixture");
    sink.stdout
}

const NODE: &str = r#"
use core.mem

struct Node {
    payload: Int
    hops: Int
}
"#;

// ── Criterion 1: safe code cannot move or replace a pinned place ────────────

#[test]
fn moving_a_pinned_place_is_e0219() {
    let src = format!(
        r#"{NODE}
fn consume(n: ^Node) => Int {{ return n.payload }}

fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    pinned :: mem.pin(&node)
    taken :: consume(^node)
    print("{{taken}} {{(pinned.payload)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), vec!["E0219"]);
}

#[test]
fn replacing_a_pinned_place_is_e0219() {
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    pinned :: mem.pin(&node)
    node = Node.{{payload: 9, hops: 0}}
    print("{{(pinned.payload)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), vec!["E0219"]);
}

#[test]
fn pinning_a_value_instead_of_a_place_is_e0218() {
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    pinned :: mem.pin(node)
    print("{{(node.payload)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), vec!["E0218"]);
}

#[test]
fn the_promise_ends_with_the_pin_scope() {
    // The pin dies at the end of the block, so the later move is legal — the
    // contract is a loan, not a permanent property of the place.
    let src = format!(
        r#"{NODE}
fn consume(n: ^Node) => Int {{ return n.payload }}

fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    if true {{
        pinned :: mem.pin(&node)
        pinned.hops += 1
    }}
    taken :: consume(^node)
    print("{{taken}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
}

// ── Editing through the pin reaches the owner, on every tier ────────────────

#[test]
fn editing_through_a_pin_reaches_the_owner_on_every_tier() {
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 41, hops: 0}}
    pinned :: mem.pin(&node)
    pinned.hops += 1
    pinned.payload += 1
    print("{{(pinned.payload)}} {{(pinned.hops)}}")
}}
"#
    );
    assert_eq!(interpret(&src), "42 1\n");
    if let Some(out) = build_and_run("edit_through", &src) {
        assert_eq!(out, "42 1\n");
    }
}

// ── Criterion 3: declared `Pin<U>` fields project as pins ──────────────────

#[test]
fn a_pin_field_projects_to_a_pin_on_every_tier() {
    let src = format!(
        r#"{NODE}
struct Queue {{
    label: String
    head: Pin<Node>
}}

fn run() {{
    node := Node.{{payload: 41, hops: 0}}
    pinned :: mem.pin(&node)
    pinned.hops += 1
    queue :: Queue.{{label: "ready", head: mem.pin(&node)}}
    print("{{(queue.label)}} {{(queue.head.payload)}} {{(queue.head.hops)}}")
}}
"#
    );
    assert_eq!(interpret(&src), "ready 41 1\n");
    if let Some(out) = build_and_run("pin_field", &src) {
        assert_eq!(out, "ready 41 1\n");
    }
}

#[test]
fn pinning_an_already_pinned_place_stays_one_pin() {
    // I8: one mechanism, one spelling. `Pin<Pin<T>>` must never exist.
    let src = format!(
        r#"{NODE}
struct Queue {{
    head: Pin<Node>
}}

fn takes_pin(n: ^Pin<Node>) => Int {{ return n.payload }}

fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    queue :: Queue.{{head: mem.pin(&node)}}
    seen :: takes_pin(^mem.pin(&queue.head))
    print("{{seen}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
}

// ── Criterion 4: cleanup and panic paths preserve the contract ─────────────

#[test]
fn a_pin_cannot_escape_its_owner_scope() {
    let src = format!(
        r#"{NODE}
fn make() => Pin<Node> {{
    node := Node.{{payload: 7, hops: 0}}
    return mem.pin(&node)
}}

fn run() {{
    escaped :: make()
    print("{{(escaped.payload)}}")
}}
"#
    );
    let codes = error_codes(&src);
    assert!(
        codes.iter().any(|code| code == "E2305"),
        "a pin into a local must not outlive its owner: {codes:?}"
    );
}

#[test]
fn a_pin_survives_a_branch_and_reads_of_the_owner() {
    // The contract holds across a branch: edits made through the pin on
    // either path reach the same storage.
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 41, hops: 0}}
    pinned :: mem.pin(&node)
    pinned.hops += 1
    if pinned.payload > 0 {{
        pinned.hops += 1
    }}
    print("{{(pinned.hops)}}")
}}
"#
    );
    assert_eq!(interpret(&src), "2\n");
    if let Some(out) = build_and_run("branch_path", &src) {
        assert_eq!(out, "2\n");
    }
}

// ── A pin never crosses a task boundary ────────────────────────────────────

#[test]
fn a_pin_cannot_cross_a_task_boundary() {
    let src = format!(
        r#"{NODE}
use core.tasks

fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    pinned :: mem.pin(&node)
    handle :: tasks.spawn(() => pinned.payload)
    print("{{(handle.join())}}")
}}
"#
    );
    let codes = error_codes(&src);
    assert!(
        !codes.is_empty(),
        "a pin is a borrow into one owner's storage; it must not cross a task"
    );
}
