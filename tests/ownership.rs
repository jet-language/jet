//! Tests for M2 ownership / borrow transpiler rules (SAFETY DEFAULTS).

#[test]
fn implicit_clone_is_lint_not_error() {
    let src = r#"
fn consume(take s: String) {
    print(s);
}

fn main() {
    val msg: String = "hello";
    consume(msg);
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
fn touch(mut n: Int) {
    print(n);
}

fn main() {
    var x: Int = 1;
    touch(x);
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0202"));
}

#[test]
fn move_non_clonable_is_hard_error() {
    let src = r#"
fn consume(take item: NoClone) {
    print(0);
}

fn main() {
    val msg: String = "hi";
    consume(msg);
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0201"));
}

#[test]
fn view_return_transpiles_to_ref() {
    let src = r#"
fn peek(msg: String) -> view String {
    return msg;
}

fn main() {
    print(0);
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
fn bad() -> view String {
    val msg: String = "ok";
    return msg;
}

fn main() {
    print(0);
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
    print(0);
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
fn noop(h: Shared[Int]) {
    print(0);
}

fn loop_user(h: Shared[Int]) {
    loop {
        noop(h);
    }
}

fn main() {
    print(0);
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
const LIMIT = 10;

fn show(n: Int) {
    print(n);
}

fn main() {
    show(LIMIT);
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
fn both(mut a: Int, b: Int) {
    print(b);
}

fn main() {
    var x: Int = 1;
    both(mut x, x);
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0204"));
}

#[test]
fn deref_outside_unsafe_is_error() {
    let src = r#"
fn main() {
    val x: Int = 1;
    print(*x);
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0208"));
}
