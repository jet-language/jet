//! Tests for M2 ownership / borrow transpiler rules (SAFETY DEFAULTS).

#[test]
fn implicit_clone_is_lint_not_error() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn main() {
msg: String #= "hello"
    consume(msg)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.lints.iter().any(|d| d.code == "L0201"),
        "expected L0201 implicit clone lint"
    );
    assert!(out.rust.contains(".clone()"));
}

#[test]
fn mutate_required_at_call_site() {
    let src = r#"
fn touch(n: ~Int) {
    print(n)
}

fn main() {
    x: Int := 1
    touch(x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0202"));
}

#[test]
fn move_non_clonable_is_hard_error() {
    let src = r#"
fn consume(item: ^NoClone) {
    print(0)
}

fn main() {
msg: String #= "hi"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0201"));
}

#[test]
fn view_return_transpiles_to_ref() {
    let src = r#"
fn peek(msg: String) -> &String {
    return msg
}

fn main() {
    print(0)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("-> &String"),
        "view return should emit &T: {}",
        out.rust
    );
    assert!(
        !out.rust.contains("-> &'"),
        "view return should use elided lifetime, not explicit: {}",
        out.rust
    );
}

#[test]
fn view_return_local_text_is_error() {
    let src = r#"
fn bad() -> &String {
msg: String #= "ok"
    return msg
}

fn main() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0206"));
}

#[test]
fn stored_ref_generates_struct_lifetime() {
    let src = r#"
struct Holder {
    ref data: String,
}

fn main() {
    print(0)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("struct user_Holder<'src>"),
        "expected lifetime param on struct: {}",
        out.rust
    );
    assert!(
        out.rust.contains("data: &'src String"),
        "expected ref field typing: {}",
        out.rust
    );
}

#[test]
fn shared_auto_clone_in_loop_is_lint() {
    let src = r#"
fn noop(h: Shared<Int>) {
    print(0)
}

fn loop_user(h: Shared<Int>) {
    loop {
        noop(h)
    }
}

fn main() {
    print(0)
}
"#;
    let out = jet::compile(src).expect("should compile with lint");
    assert!(
        out.lints.iter().any(|d| d.code == "L0202"),
        "expected L0202 loop auto-clone lint"
    );
    assert!(out.rust.contains("Arc::clone"));
}

#[test]
fn const_address_taken_emits_static() {
    let src = r#"
const LIMIT = 10

fn show(n: Int) {
    print(n)
}

fn main() {
    show(LIMIT)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("static USER_LIMIT"),
        "address-taken const should emit static: {}",
        out.rust
    );
}

#[test]
fn same_call_mut_and_read_is_error() {
    let src = r#"
fn both(a: ~Int, b: Int) {
    print(b)
}

fn main() {
    x: Int := 1
    both(~x, x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0204"));
}

/// D-L0201 liveness gate: when the value is still used after the call
/// the clone is necessary and L0201 must be silent.
#[test]
fn implicit_clone_silent_when_value_live_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn main() {
msg: String #= "hello"
    consume(msg)
    print(msg)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.lints.iter().all(|d| d.code != "L0201"),
        "L0201 must be silent when value is still used after the call"
    );
    // The clone is still generated (it's necessary), just no warning.
    assert!(out.rust.contains(".clone()"), "clone must still be emitted");
}

/// D-L0201 liveness gate: when the value IS dead after the call,
/// L0201 still fires (the clone is wasteful, `move` would be better).
#[test]
fn implicit_clone_fires_when_value_dead_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn main() {
msg: String #= "hello"
    consume(msg)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.lints.iter().any(|d| d.code == "L0201"),
        "L0201 must fire when value is dead after the call"
    );
}

#[test]
fn deref_outside_unsafe_is_error() {
    let src = r#"
fn main() {
x: Int #= 1
    print(*x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0208"));
}

/// D-L0201 liveness gate: clone inside a nested `if` block must NOT fire L0201
/// when the value is used in the enclosing block after the `if`.
/// Previously `is_name_live_after` only checked the current block's tail and
/// missed uses in enclosing scopes — a false-fire that would advise `move msg`
/// but the move would cause use-after-move on the `print(msg)` below.
#[test]
fn implicit_clone_silent_when_live_in_enclosing_block() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) -> Bool { return b }

fn main() {
msg: String #= "hello"
    if maybe(true) {
        consume(msg)
    }
    print(msg)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.lints.iter().all(|d| d.code != "L0201"),
        "L0201 must be silent when value is used in the enclosing block after the if"
    );
    assert!(out.rust.contains(".clone()"), "clone must still be emitted");
}

/// D-L0201 liveness gate: clone inside a nested block where the value is
/// genuinely dead everywhere after (enclosing block included) still fires.
#[test]
fn implicit_clone_fires_when_dead_in_all_enclosing_blocks() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) -> Bool { return b }

fn main() {
msg: String #= "hello"
    if maybe(true) {
        consume(msg)
    }
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.lints.iter().any(|d| d.code == "L0201"),
        "L0201 must fire when value is dead on all paths after the nested block"
    );
}
