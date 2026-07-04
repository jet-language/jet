//! Tests for M2 ownership / borrow transpiler rules (SAFETY DEFAULTS).

/// D-MEM1/S2: no clone is ever silent (I8) — the former lint (`L0201`) is now
/// a hard error (`E0209`), regardless of liveness.
#[test]
fn implicit_clone_is_error_not_lint() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg: String :: "hello"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(
        diags.iter().any(|d| d.code == "E0209"),
        "expected E0209 implicit-clone hard error"
    );
}

/// D-MEM1/S2 ("signatures can't lie"): an unmarked param is always `Read` —
/// no body-usage elevation. A body write through it is a hard error (E0205)
/// with a fix-it naming the `&` sigil, same as a non-`&self` receiver.
#[test]
fn body_write_to_unmarked_param_is_error() {
    let src = r#"
struct Counter {
    n: Int
}
fn bump(c: Counter) {
    c.n = c.n + 1
}
fn run() {
    c: Counter :: Counter.{ n: 0 }
    bump(c)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0205")
        .expect("expected E0205: body write to an unmarked (Read) param");
    assert!(
        d.fix.contains("&Counter"),
        "fix should point at adding `&` to the param, got: {}",
        d.fix
    );
}

#[test]
fn mutate_required_at_call_site() {
    let src = r#"
fn touch(n: &Int) {
    print(n)
}

fn run() {
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

fn run() {
msg: String :: "hi"
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

fn run() {
    print(0)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("-> &String"),
        "view return should emit &T: {}",
        out.rust
    );
    // User-facing signatures must elide lifetimes; prelude helpers (e.g.
    // jet_view_new) legitimately carry explicit ones, so scope the check to
    // generated user_ functions.
    for line in out.rust.lines().filter(|l| l.contains("fn user_")) {
        assert!(
            !line.contains("&'"),
            "view return should use elided lifetime, not explicit: {line}"
        );
    }
}

#[test]
fn view_return_local_text_is_error() {
    let src = r#"
fn bad() -> &String {
msg: String :: "ok"
    return msg
}

fn run() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0206"));
}

#[test]
fn ref_owner_param_fill_compiles() {
    let src = r#"
struct TokenView {
    text: &String
}
fn show(source: String) {
    t: TokenView :: TokenView.{ text: source }
    print(t.text)
}
fn run() {
    show("hello")
}
"#;
    jet::compile(src).expect("inferred-owner `&String` field should compile");
}

#[test]
fn stored_ref_generates_struct_lifetime() {
    let src = r#"
struct Holder {
    data: &String,
}

fn run() {
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

fn run() {
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

fn run() {
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
fn both(a: &Int, b: Int) {
    print(b)
}

fn run() {
    x: Int := 1
    both(&x, x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0204"));
}

/// E0209 liveness gate (was D-L0201): when the value is still used after the
/// call, `^` would break that later use — E0209 still fires (hard error), but
/// the fix menu offers `.clone()`/reorder instead of `^`.
#[test]
fn implicit_clone_errors_with_reorder_menu_when_live_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg: String :: "hello"
    consume(msg)
    print(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("reorder"),
        "live-after fix menu should suggest reordering, got: {}",
        d.fix
    );
}

/// E0209 liveness gate (was D-L0201): when the value IS dead after the call,
/// `^` is safe (this is its last use) — the fix menu leads with `^`.
#[test]
fn implicit_clone_errors_with_move_menu_when_dead_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg: String :: "hello"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("^msg"),
        "dead-after fix menu should lead with `^msg`, got: {}",
        d.fix
    );
}

#[test]
fn deref_outside_unsafe_is_error() {
    let src = r#"
fn run() {
x: Int :: 1
    print(*x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0208"));
}

/// E0209 liveness gate (was D-L0201): a clone inside a nested `if` block gets
/// the reorder/copy menu (not the `^`-leads menu) when the value is used in
/// the enclosing block after the `if`. `is_name_live_after` checks the
/// current block's tail AND all enclosing scopes — missing enclosing scopes
/// would wrongly advise `^msg` here, which would use-after-move the
/// `print(msg)` below.
#[test]
fn implicit_clone_uses_reorder_menu_when_live_in_enclosing_block() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) -> Bool { return b }

fn run() {
msg: String :: "hello"
    if maybe(true) {
        consume(msg)
    }
    print(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("reorder"),
        "live-in-enclosing-block fix menu should suggest reordering, got: {}",
        d.fix
    );
}

/// E0209 liveness gate (was D-L0201): a clone inside a nested block where the
/// value is genuinely dead everywhere after (enclosing block included) gets
/// the `^`-leads menu.
#[test]
fn implicit_clone_uses_move_menu_when_dead_in_all_enclosing_blocks() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) -> Bool { return b }

fn run() {
msg: String :: "hello"
    if maybe(true) {
        consume(msg)
    }
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("^msg"),
        "dead-in-all-enclosing-blocks fix menu should lead with `^msg`, got: {}",
        d.fix
    );
}
