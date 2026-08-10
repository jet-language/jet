mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};

const STALE_SOURCE: &str = r#"
fn run() {
    pool := Pool<String>.new()
    stale :: pool.add("first")
    removed :: pool.remove(stale)
    live :: pool.add("second")
    print(stale == live)
}
"#;

#[test]
fn pool_ids_are_equatable_without_requiring_the_payload_to_be_equatable() {
    let source = r#"
struct Payload {
    callback: fn(Int) => Int
}

fn identity(value: Int) => Int {
    return value
}

fn equal<T: Equatable>(left: T, right: T) => Bool {
    return left == right
}

fn run() {
    pool := Pool<Payload>.new()
    left :: pool.add(Payload.{callback: identity})
    right :: pool.add(Payload.{callback: identity})
    same :: equal(left, right)
    print(same)
}
"#;

    let result = jet::compile(source);
    assert!(
        result.is_ok(),
        "Id<T> equality must depend on Id identity, not T: {result:#?}"
    );
}

#[test]
fn reused_pool_slot_does_not_equal_its_stale_generation() {
    if have_rustc() {
        let (code, stdout) = build_and_run("pool_id_stale_generation", STALE_SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, "false\n");
    }

    let (code, stdout, stderr) =
        run_default_multi("pool_id_stale_generation", "main.jet", &[("main.jet", STALE_SOURCE)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\n");
}
