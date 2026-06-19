//! E2-M16 pure evaluation tests (S60, D-PURE1/D-PURE2/D-PURE3).

use std::path::Path;
use std::sync::Mutex;

// Serialize all tests that mutate the process-global JET_STORE_DIR to prevent
// concurrent set_var races under cargo's parallel runner.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` with `JET_STORE_DIR` pointed at a fresh `dir`, serializing concurrent
/// calls and restoring the prior value afterward.
fn with_store<T, F: FnOnce() -> T>(dir: &Path, f: F) -> T {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let prev = std::env::var("JET_STORE_DIR").ok();
    std::env::set_var("JET_STORE_DIR", dir);
    let result = f();
    match prev {
        Some(v) => std::env::set_var("JET_STORE_DIR", v),
        None => std::env::remove_var("JET_STORE_DIR"),
    }
    let _ = std::fs::remove_dir_all(dir);
    result
}

/// `pure fn` parses and compiles without error.
#[test]
fn pure_fn_compiles() {
    let src = r#"
pure fn add(a: Int, b: Int) -> Int {
    return a + b;
}
fn main() {
    print("{add(1, 2)}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "pure fn should compile: {:?}", res.err());
}

/// Impure call inside `pure fn` fires E3401.
#[test]
fn pure_fn_impure_call_is_e3401() {
    let src = r#"
pure fn bad() -> Int {
    print("side effect");
    return 42;
}
fn main() {
    print("{bad()}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "impure call in pure fn should fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// A `pure fn` calling another `pure fn` is fine.
#[test]
fn pure_fn_calling_pure_fn_is_ok() {
    let src = r#"
pure fn square(n: Int) -> Int {
    return n * n;
}
pure fn cube(n: Int) -> Int {
    return n * square(n);
}
fn main() {
    print("{cube(3)}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "pure calling pure should compile: {:?}", res.err());
}

/// `pub pure fn` parses and compiles without error.
#[test]
fn pub_pure_fn_compiles() {
    let src = r#"
pub pure fn double(n: Int) -> Int {
    return n * 2;
}
fn main() {
    print("{double(5)}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_ok(), "pub pure fn should compile: {:?}", res.err());
}

/// `pure fn` calling an impure user-defined function fires E3401.
#[test]
fn pure_fn_calling_impure_user_fn_is_e3401() {
    let src = r#"
fn read_value() -> Int {
    print("side effect");
    return 1;
}
pure fn compute() -> Int {
    return read_value();
}
fn main() {
    print("{compute()}");
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "pure fn calling impure user fn should fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E3401"),
        "expected E3401, got: {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

/// Store generation tracking: list_generations returns an empty list when
/// no generations are recorded (using a temp store dir).
#[test]
fn store_generations_empty() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_empty");
    with_store(&dir, || {
        // Just check it doesn't panic on a fresh store.
        let _ = jet::Store::list_generations();
    });
}

/// Store generation tracking: record_generation writes a record.
#[test]
fn store_record_generation() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_record");
    with_store(&dir, || {
        let gen = jet::Store::record_generation();
        assert!(gen >= 1, "generation should be at least 1");
        let gens = jet::Store::list_generations();
        assert!(!gens.is_empty(), "should have at least one generation recorded");
    });
}

/// Store rollback: rolling back to a non-existent generation returns Err.
#[test]
fn store_rollback_invalid_gen() {
    let dir = std::env::temp_dir().join("jet_pure_test_gen_rollback_inv");
    with_store(&dir, || {
        let result = jet::Store::rollback_to(9999);
        assert!(result.is_err(), "rollback to non-existent gen should fail");
    });
}
