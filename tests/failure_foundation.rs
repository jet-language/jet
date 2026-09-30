//! D-FAILURE-FOUNDATION1=A: behavioral contract matrix for the one failure rail.

mod common;
use std::fs;

#[path = "tir_support/mod.rs"]
mod tir_support;

const CONTRACT_MATRIX: &str = r#"
#Error
enum TypedFailure {
    Bad
}

#Error
enum OtherFailure {
    Worse
}

#Error
enum StoreFailure {
    Missing
}

// Omitted contract: the effective route is Int !Err.
fn implicit(value: Int) -> Int {
    if value == 0 {
        return Err("implicit")
    }
    return value
}

// Expert opt-out: a named error domain.
fn explicit(value: Int) -> Int TypedFailure! {
    if value == 0 {
        return Err(TypedFailure.Bad)
    }
    return value
}

// Expert opt-out: an error union widens a member failure.
fn union(value: Int) -> Int (TypedFailure | OtherFailure)! {
    if value == 0 {
        return Err(TypedFailure.Bad)
    }
    return value
}

// One declared conversion crosses from a named error into default Err.
impl StoreFailure -> Err {
    return Err("converted")
}

fn converted(value: Int) -> Int StoreFailure! {
    if value == 0 {
        return Err(StoreFailure.Missing)
    }
    return value
}

fn contextual_source() -> Int -> Err("context", code: "E_CONTEXT", cause: Err("root"))

// `?(text)` keeps the original structured error while adding one context hop.
fn contextual() -> Int -> contextual_source()?("loading")

// Optional success still rides the Result-shaped carrier.
fn optional_success(value: Int) -> Int? {
    if value == 0 {
        return None
    }
    return Val(value)
}

// Unit success still propagates a failure through the same carrier.
fn unit_success(fail: Bool) {
    if fail {
        return Err("unit")
    }
}

fn unit_caller(fail: Bool) -> Int {
    unit_success(fail)
    return 7
}

// Function values retain their fallible contract.
fn apply(callback: fn(Int) -> Int, value: Int) -> Int -> callback(value)

// Generic call results use the same automatic propagation rule.
fn generic_forward<T>(value: T) -> T -> value

fn generic_caller(value: Int) -> Int -> generic_forward<Int>(implicit(value))

// No reachable failure: the !Never proof is a valid contract.
fn impossible() -> Int Never! -> 7

fn run() {
    print(implicit(2) ?? -1)
    print(implicit(0) ?? -1)
    print(explicit(2) ?? -2)
    print(explicit(0) ?? -2)
    print(union(2) ?? -3)
    print(union(0) ?? -3)
    print(converted(2) ?? -4)
    print(converted(0) ?? -4)
    print(contextual() ?? -5)
    print(optional_success(2) ?? -6)
    print(optional_success(0) ?? -6)
    print(unit_caller(false) ?? -7)
    print(unit_caller(true) ?? -7)
    print(apply(implicit, 2) ?? -8)
    print(apply(implicit, 0) ?? -8)
    print(generic_caller(2) ?? -9)
    print(generic_caller(0) ?? -9)
    print(impossible() ?? -10)
}
"#;

#[test]
fn failure_contract_matrix_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "failure_contract_matrix",
        CONTRACT_MATRIX,
        "2\n-1\n2\n-2\n2\n-3\n2\n-4\n-5\n2\n-6\n7\n-7\n2\n-8\n2\n-9\n7\n",
    );
}

#[test]
fn direct_struct_error_match_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "direct_struct_error_match",
        include_str!("../Examples/features/errors/direct_struct_error_match.jet"),
        "ok: 7\nerr: empty input\n",
    );
}

/// D10 (S80, D-DISPLAYDBG1): the default `Err` shows its message through
/// bare interpolation, both as a match binding and as the fallback's `err`.
#[test]
fn default_err_display_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "default_err_display",
        r#"
fn fails(n: Int) -> Int Err! {
    if n < 0 -> return Err("negative: {n}")
    Ok(n)
}

fn run() {
    a :: fails(-1) ?? {
        print("fallback: {err}")
        0
    }
    print(a)
    if fails(-2) == {
        .Ok(n) -> print(n)
        .Err(e) -> print("failed: {e}")
    }
}
"#,
        "fallback: negative: -1\n0\nfailed: negative: -2\n",
    );
}

/// D1 / D8: an else-less statement match over a union of `#Error struct`
/// members is exhaustive, and an empty sibling list takes its element type
/// from the other items.
#[test]
fn union_error_struct_match_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "union_error_struct_match",
        r#"
#Error
struct ListWasEmpty {
    message: String
}

#Error
struct BadNumStr {
    message: String
}

fn first(xs: [String]) -> String ListWasEmpty! {
    if xs.is_empty() -> return Err(ListWasEmpty{message: "empty"})
    Ok(xs[0])
}

fn to_num(s: String) -> Int BadNumStr! {
    s.to_int() ?? return Err(BadNumStr{message: "nan"})
}

fn increment_first(xs: [String]) -> Int (ListWasEmpty | BadNumStr)! {
    s :: first(xs)
    n :: to_num(s)
    Ok(n + 1)
}

fn run() {
    loop xs in [["41"], [], ["x"]] {
        if increment_first(xs) == {
            .Ok(n) -> print(n)
            .Err(.ListWasEmpty(_)) -> print("empty")
            .Err(.BadNumStr(_)) -> print("not a number")
        }
    }
}
"#,
        "42\nempty\nnot a number\n",
    );
}

/// D3 (D-MEMO1=A, D-EFFECT-OMIT1=A): `#Memo` accepts an inferred-pure function.
#[test]
fn memo_inferred_pure_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "memo_inferred_pure",
        "#Memo fn slow_square(n: Int) -> Int { n * n }\n\nfn run() {\n    print(slow_square(12))\n    print(slow_square(12))\n    print(slow_square(13))\n}\n",
        "144\n144\n169\n",
    );
}

#[test]
fn optional_success_result_fallback_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree_with_application_policy(
        "optional_success_result_fallback",
        r#"
#Error
enum TestFailure {
    Missing
}

fn optional_success_result(value: Int) -> Int? TestFailure! {
    if value == -1 {
        return Err(TestFailure.Missing)
    }
    if value == 0 {
        return None
    }
    return Ok(Val(value))
}

fn run() {
    print(optional_success_result(8080) ?? 0)
    print(optional_success_result(0) ?? 0)
    print(optional_success_result(-1) ?? 0)
}
"#,
        "8080\n0\n0\n",
        "name: \"optional_success_result_fallback\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
    );
}

#[test]
fn never_contract_rejects_every_reachable_failure_route() {
    let cases = [
        (
            "explicit propagation",
            r#"
fn fail() -> Int -> Err("bad")
fn impossible() -> Int Never! -> fail()?("unreachable")
fn run() {}
"#,
        ),
        (
            "implicit direct return",
            r#"
fn fail() -> Int -> Err("bad")
fn impossible() -> Int Never! -> fail()
fn run() {}
"#,
        ),
        (
            "implicit statement propagation",
            r#"
fn fail() -> Int -> Err("bad")
fn impossible() Never! {
    fail()
}
fn run() {}
"#,
        ),
        (
            "implicit branch propagation",
            r#"
fn fail() -> Int -> Err("bad")
fn impossible(value: Bool) -> Int Never! {
    if value {
        fail()
    }
    return 7
}
fn run() {}
"#,
        ),
    ];
    let scratch = common::Scratch::new("failure-never");
    let path = scratch.join("failure_never.jet");
    let shown = path.to_string_lossy().into_owned();
    for (case, source) in cases {
        fs::write(&path, source).expect("write !Never fixture");
        let diagnostics = jet::compile_with_path(source, &shown)
            .expect_err("!Never must reject a reachable failure");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E2404"),
            "{case}: expected E2404, got {diagnostics:?}"
        );
    }

    let empty_path = scratch.join("failure_never_empty.jet");
    let empty_shown = empty_path.to_string_lossy().into_owned();
    fs::write(&empty_path, "fn impossible() Never! {}\nfn run() {}\n")
        .expect("write empty !Never fixture");
    jet::compile_with_path(
        "fn impossible() Never! {}\nfn run() {}\n",
        &empty_shown,
    )
    .expect("a !Never function with no reachable failure can fall through as success");
}

#[test]
fn never_call_in_value_branch_agrees_across_execution_tiers() {
    tir_support::assert_tiers_agree(
        "never_call_in_value_branch",
        r#"
fn stop(message: String) {
    panic(message)
}

fn load_name(found: Bool) -> String {
    return if found -> { "Ada" } else -> { stop("name missing") }
}

fn run() {
    print(load_name(true))
}
"#,
        "Ada\n",
    );
}
