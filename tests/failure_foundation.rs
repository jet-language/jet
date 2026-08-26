//! D-FAILURE-FOUNDATION1=A: behavioral contract matrix for the one failure rail.

mod common;

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
fn implicit(value: Int) Int -> {
    if value == 0 {
        return Err("implicit")
    }
    return value
}

// Expert opt-out: a named error domain.
fn explicit(value: Int) Int !TypedFailure -> {
    if value == 0 {
        return Err(TypedFailure.Bad)
    }
    return value
}

// Expert opt-out: an error union widens a member failure.
fn union(value: Int) Int !(TypedFailure | OtherFailure) -> {
    if value == 0 {
        return Err(TypedFailure.Bad)
    }
    return value
}

// One declared conversion crosses from a named error into default Err.
impl StoreFailure -> Err {
    return Err("converted")
}

fn converted(value: Int) Int !StoreFailure -> {
    if value == 0 {
        return Err(StoreFailure.Missing)
    }
    return value
}

fn contextual_source() Int -> Err("context", code: "E_CONTEXT", cause: Err("root"))

// `?(text)` keeps the original structured error while adding one context hop.
fn contextual() Int -> contextual_source()?("loading")

// Optional success still rides the Result-shaped carrier.
fn optional_success(value: Int) ?Int ! -> {
    if value == 0 {
        return None
    }
    return Val(value)
}

// Unit success still propagates a failure through the same carrier.
fn unit_success(fail: Bool) ! {
    if fail {
        return Err("unit")
    }
}

fn unit_caller(fail: Bool) Int -> {
    unit_success(fail)
    return 7
}

// Function values retain their fallible contract.
fn apply(callback: fn(Int) Int !, value: Int) Int ! -> callback(value)

// Generic call results use the same automatic propagation rule.
fn generic_forward<T>(value: T) T ! -> value

fn generic_caller(value: Int) Int -> generic_forward<Int>(implicit(value))

// No reachable failure: the !Never proof is a valid contract.
fn impossible() Int !Never -> 7

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
fn never_contract_rejects_a_reachable_failure() {
    let source = r#"
fn fail() Int -> Err("bad")
fn impossible() Int !Never -> fail()?("unreachable")
fn run() {}
"#;
    let diagnostics = jet::compile_with_path(source, "failure_never.jet")
        .expect_err("!Never must reject a reachable failure");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E2404"),
        "expected E2404, got {diagnostics:?}"
    );
}
