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
/// checker. Pass `allow_authored_unsafe` when the Jet source contains an
/// audited `#Unsafe("…")` region that must lower to Rust `unsafe`.
fn build_and_run(name: &str, src: &str, allow_authored_unsafe: bool) -> Option<String> {
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
    if !allow_authored_unsafe {
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
    }

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
    if let Some(out) = build_and_run("edit_through", &src, false) {
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
    if let Some(out) = build_and_run("pin_field", &src, false) {
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
    if let Some(out) = build_and_run("branch_path", &src, false) {
        assert_eq!(out, "2\n");
    }
}

#[test]
fn pinning_a_field_place_records_the_field_not_a_copy() {
    // Regression: the auto-copy pass wrapped a field argument in `copy`, so the
    // pin silently promised address stability for a temporary and the owner
    // field stayed replaceable.
    let src = format!(
        r#"{NODE}
struct Pair {{
    left: Node
    right: Node
}}

fn run() {{
    pair := Pair.{{left: Node.{{payload: 1, hops: 0}}, right: Node.{{payload: 2, hops: 0}}}}
    first :: mem.pin(&pair.left)
    pair.left = Node.{{payload: 9, hops: 0}}
    print("{{(first.hops)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), vec!["E0219"]);
}

#[test]
fn pinning_an_index_place_records_the_element() {
    let src = format!(
        r#"{NODE}
fn run() {{
    nodes := [Node.{{payload: 1, hops: 0}}, Node.{{payload: 2, hops: 0}}]
    first :: mem.pin(&nodes[0])
    first.hops += 1
    print("{{(first.hops)}} {{(first.payload)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
    assert_eq!(interpret(&src), "1 1\n");
    if let Some(out) = build_and_run("index_pin", &src, false) {
        assert_eq!(out, "1 1\n");
    }
}

#[test]
fn two_pins_on_sibling_fields_do_not_conflict() {
    // Sibling places never overlap, so pinning both is one contract per field,
    // not a double borrow. Only nesting is exempt from the overlap rule.
    let src = format!(
        r#"{NODE}
struct Pair {{
    left: Node
    right: Node
}}

fn run() {{
    pair := Pair.{{left: Node.{{payload: 1, hops: 0}}, right: Node.{{payload: 2, hops: 0}}}}
    first :: mem.pin(&pair.left)
    second :: mem.pin(&pair.right)
    first.hops += 1
    second.hops += 2
    print("{{(first.hops)}} {{(second.hops)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
    assert_eq!(interpret(&src), "1 2\n");
    if let Some(out) = build_and_run("sibling_pins", &src, false) {
        assert_eq!(out, "1 2\n");
    }
}

#[test]
fn a_pin_taken_each_iteration_is_a_fresh_loan() {
    // The loan ends with the loop body's scope, so the next iteration may pin
    // the same place again without tripping the overlap rule.
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 0, hops: 0}}
    loop i, 0..2 {{
        pinned :: mem.pin(&node)
        pinned.hops += 1
    }}
    print("{{(node.hops)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
    assert_eq!(interpret(&src), "3\n");
    if let Some(out) = build_and_run("loop_pin", &src, false) {
        assert_eq!(out, "3\n");
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

// ── Card #1360: returned aggregates may store write windows via `from` ──────

#[test]
fn a_library_may_return_a_pin_field_from_an_owner_parameter() {
    let src = format!(
        r#"{NODE}
struct Queue {{
    label: String
    head: Pin<Node>
}}

fn attach(label: String, node: &Node) => Queue from node {{
    return Queue.{{label: ~label, head: mem.pin(&node)}}
}}

fn run() {{
    node := Node.{{payload: 41, hops: 0}}
    queue :: attach("ready", &node)
    queue.head.hops += 1
    queue.head.payload += 1
    print("{{(queue.label)}} {{(queue.head.payload)}} {{(queue.head.hops)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), Vec::<String>::new());
    assert_eq!(interpret(&src), "ready 42 1\n");
    if let Some(out) = build_and_run("return_pin_field", &src, false) {
        assert_eq!(out, "ready 42 1\n");
    }
}

// ── Card #1361: reading the owner beside a live write window is E0220 ───────

#[test]
fn reading_the_owner_while_a_pin_is_live_is_e0220() {
    let src = format!(
        r#"{NODE}
fn run() {{
    node := Node.{{payload: 41, hops: 0}}
    pinned :: mem.pin(&node)
    if node.payload > 0 {{
        pinned.hops += 1
    }}
    print("{{(pinned.hops)}}")
}}
"#
    );
    assert_eq!(error_codes(&src), vec!["E0220"]);
}

// ── Criterion 2: local #Unsafe, safe Pin API at call sites ─────────────────

#[test]
fn library_wires_self_ref_under_unsafe_and_exposes_safe_pin_api() {
    // The audited region stays inside the library. Callers of `wire_self` never
    // write `#Unsafe` — they receive a ready `Pin<SelfNode>` (D-PIN1 step 5).
    let src = r#"
use core.mem

struct SelfNode {
    payload: Int
    self_addr: Int
}

fn wire_self(node: &SelfNode) => Pin<SelfNode> {
    #Unsafe("node storage is fixed for the returned pin; self_addr names this place") {
        node.self_addr = mem.address_of(node.payload)
    }
    return mem.pin(&node)
}

fn run() {
    node := SelfNode.{payload: 7, self_addr: 0}
    pinned :: wire_self(&node)
    pinned.payload += 1
    print("{(pinned.payload)} {(pinned.self_addr != 0)}")
}
"#;
    assert_eq!(error_codes(src), Vec::<String>::new());
    assert_eq!(interpret(src), "8 true\n");
    if let Some(out) = build_and_run("safe_pin_api", src, true) {
        assert_eq!(out, "8 true\n");
    }
}

// ── Criterion 4: cleanup / panic / cancel keep the contract ────────────────

#[test]
fn automatic_cleanup_still_runs_when_a_pin_is_live_at_panic() {
    // Owner destruction / panic must not skip resource cleanup, and the pin
    // must not outlive the place it names (D-PIN1 criterion 4).
    let src = r#"
use core.mem

struct Guard {
    name: String
}

impl Guard.Close {
    fn close(^self) {
        print("closed {self.name}")
    }
}

struct Node {
    payload: Int
    hops: Int
}

fn run() {
    guard := Guard.{name: "pin"}
    node := Node.{payload: 1, hops: 0}
    pinned :: mem.pin(&node)
    pinned.hops += 1
    print("body {(pinned.hops)}")
    panic("stop")
}
"#;
    assert_eq!(error_codes(src), Vec::<String>::new());
    if let Some(out) = build_and_run("pin_panic_cleanup", src, false) {
        assert!(
            out.contains("body 1") && out.contains("closed pin"),
            "panic must still run automatic cleanup while a pin is live: {out:?}"
        );
    }
}

#[test]
fn cancelling_a_task_cannot_smuggle_a_pin_across_the_boundary() {
    // Cancellation ends the task; a pin is a borrow into one owner's storage
    // and must not become a cross-task escape hatch (criterion 4 + task law).
    let src = format!(
        r#"{NODE}
use core.tasks

fn run() {{
    node := Node.{{payload: 7, hops: 0}}
    pinned :: mem.pin(&node)
    handle :: tasks.spawn(() => {{
        pinned.hops += 1
        return pinned.payload
    }})
    handle.cancel()
    print("{{(handle.join())}}")
}}
"#
    );
    let codes = error_codes(&src);
    assert!(
        !codes.is_empty(),
        "a live pin must not cross into a cancellable task body: {codes:?}"
    );
}

// ── Criterion 5: Fixed / arena / caller-storage pin parity ─────────────────

#[test]
fn pinning_fixed_and_arena_storage_keeps_edits_in_caller_owned_places() {
    let src = r#"
use core.mem

struct Node {
    payload: Int
    hops: Int
}

fn run() {
    fixed :: mem.Fixed.new(size: 256)
    fixed_node :: fixed.alloc(Node.{payload: 3, hops: 0})
    fixed_pin :: mem.pin(&fixed_node)
    fixed_pin.hops += 1

    arena :: mem.Arena.new(capacity: 1024)
    arena_node :: arena.alloc(Node.{payload: 5, hops: 0})
    arena_pin :: mem.pin(&arena_node)
    arena_pin.hops += 2

    print("{(fixed_pin.payload)} {(fixed_pin.hops)} {(arena_pin.payload)} {(arena_pin.hops)}")
    close(^fixed)
    close(^arena)
}
"#;
    assert_eq!(error_codes(src), Vec::<String>::new());
    assert_eq!(interpret(src), "3 1 5 2\n");
    if let Some(out) = build_and_run("fixed_arena_pin", src, false) {
        assert_eq!(out, "3 1 5 2\n");
    }
}
