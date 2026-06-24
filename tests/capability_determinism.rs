//! D-CAP8 capability inference — determinism and correctness tests.
//!
//! Verifies that:
//!   1. Repeated compilations of the same source produce byte-identical output
//!      (no HashMap-iteration-order nondeterminism).
//!   2. Unmarked params infer Write when field-assigned, Move when passed as `^`
//!      to a Move-param callee, and Read when only read.

/// Source with several inferred-capability params: write, read, and move.
///
/// - `heal(p: Player)` — body assigns `p.hp`, infers Write (~Player)
/// - `name_of(p: Player)` — body only reads, infers Read
/// - `drain(t: Token)` — body passes `^t` to consume (^Token), infers Move
///
/// The main function exercises all three with the correct call-site sigils so
/// the capability checks pass.
const MULTI_INFER_SRC: &str = r#"
struct Player { hp: Int, name: String }
struct Token { id: Int }

fn heal(p: Player) {
    p.hp = p.hp + 10
}

fn name_of(p: Player) -> String {
    return p.name
}

fn consume(t: ^Token) {
    print(t.id)
}

fn drain(t: Token) {
    consume(^t)
}

fn main() {
    p := Player { hp: 90, name: "Aria" }
    heal(~p)
    print(name_of(p))
    tok := Token { id: 7 }
    drain(^tok)
}
"#;

/// Compile the same multi-infer source 50 times and assert the generated Rust
/// is byte-identical on every iteration. Catches HashMap-iteration-order
/// nondeterminism in the capability resolver.
#[test]
fn inference_is_deterministic() {
    let first = jet::compile(MULTI_INFER_SRC)
        .expect("multi-infer source compiles")
        .rust;
    for i in 1..50 {
        let out = jet::compile(MULTI_INFER_SRC)
            .unwrap_or_else(|_| panic!("compile failed on iteration {i}"))
            .rust;
        assert_eq!(
            first, out,
            "generated Rust differed on iteration {i} — HashMap iteration order nondeterminism?"
        );
    }
}

/// Unmarked param that is field-assigned (`p.hp = …`) must infer Write.
/// The call site uses `~p`; if inference resolved to Read the checker would
/// reject the `~` at the call site (E0205).
#[test]
fn unmarked_mutated_param_infers_write() {
    let src = r#"
struct Player { hp: Int }

fn heal(p: Player) {
    p.hp = p.hp + 1
}

fn main() {
    p := Player { hp: 100 }
    heal(~p)
    print(p.hp)
}
"#;
    let out = jet::compile(src).expect("heal with ~p should compile");
    // The inferred ~Player should produce a mutable reference in the output.
    assert!(
        out.rust.contains("&mut user_Player") || out.rust.contains("mut p:"),
        "expected write/mut in generated Rust: {}",
        out.rust
    );
}

/// Unmarked param that is only read must infer Read (plain borrow / copy).
/// A bare call without `~` or `^` must compile cleanly.
#[test]
fn unmarked_read_param_stays_read() {
    let src = r#"
struct Player { hp: Int, name: String }

fn name_of(p: Player) -> String {
    return p.name
}

fn main() {
    p := Player { hp: 100, name: "Aria" }
    print(name_of(p))
}
"#;
    jet::compile(src).expect("read-only param with bare call should compile");
}

/// Unmarked param passed as `^param` to a Move-param callee must infer Move.
/// The call site uses `^tok`; if inference resolved to Read the checker would
/// reject the move at the call site.
#[test]
fn unmarked_param_moved_infers_move() {
    let src = r#"
struct Token { id: Int }

fn consume(t: ^Token) {
    print(t.id)
}

fn drain(t: Token) {
    consume(^t)
}

fn main() {
    tok := Token { id: 42 }
    drain(^tok)
    print("done")
}
"#;
    jet::compile(src).expect("drain with ^tok should compile after move inference");
}
