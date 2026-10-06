//! #353: deterministic accepts-invalid and miscompile adversary corpus.

const SUITE: &str = "sema_soundness";
mod common;
include!("sema_soundness_parts/support.rs");
include!("sema_soundness_parts/metadata.rs");

#[test]
fn knowledge_loss_requires_a_spelled_gate() {
    fn rejects(source: &str, code: &str) {
        let diagnostics = jet::compile(source).expect_err("source must be rejected by sema");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "expected {code}, got {diagnostics:#?}"
        );
    }

    // Exactness: narrowing an approximate representation needs a named
    // destination-owned conversion instead of an implicit loss.
    rejects(
        r#"
fn accept_f32(value: F32) {}

fn run() {
    value :: Float{1.0}
    accept_f32(value)
}
"#,
        "E0112",
    );

    // Range: arithmetic returns the carrier only at a written bounded gate.
    rejects(
        r#"
#Numeric Severity :: distinct Int(0..10)

fn run() {
    left :: Severity.from_int(4)
    right :: Severity.from_int(5)
    total :: left + right
    print(total)
}
"#,
        "E0156",
    );

    // Range: a proof attached to an inline carrier cannot be erased by an
    // otherwise lossless-looking parameter boundary.
    rejects(
        r#"
fn accept_int(value: Int) {}

fn run() {
    bounded :: Int(0..10).from_int(4)
    accept_int(bounded)
}
"#,
        "E0156",
    );

    // Range: unary arithmetic also leaves the proven interval.
    rejects(
        r#"
fn run() {
    bounded :: Int(0..10).from_int(4)
    negated :: -bounded
    print(negated)
}
"#,
        "E0156",
    );

    // The other knowledge planes retain their existing sema-owned gates.
    rejects(include_str!("ui/quantity_implicit_rounding.jet"), "E0127");
    rejects(include_str!("ui/typestate_wrong_state.jet"), "E0150");
    rejects(include_str!("ui/taint_sink_unsanitized.jet"), "E0721");
}

#[test]
fn inline_range_runtime_conversion_requires_try() {
    let diagnostics = jet::compile(include_str!("ui/inline_range_runtime_needs_try.jet"))
        .expect_err("runtime inline range conversion must be rejected");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0136"),
        "expected E0136, got {diagnostics:#?}"
    );
}

#[test]
fn compound_updates_require_their_operator() {
    for (source, code, count) in [
        (include_str!("ui/compound_non_arithmetic_place.jet"), "E0109", 6),
        (include_str!("ui/operator_missing_compound_hook.jet"), "E0360", 6),
    ] {
        let diagnostics = jet::compile(source).expect_err("undefined compound operator");
        assert_eq!(
            diagnostics.iter().filter(|diagnostic| diagnostic.code == code).count(),
            count,
            "{diagnostics:#?}"
        );
    }
    jet::compile(
        r#"
struct Point {
    x: Int
}

impl Point.Add {
    fn add(self, rhs: Point) -> Point { return Point{x: self.x + rhs.x} }
}

fn run() {
    point := Point{x: 1}
    point += Point{x: 2}
    count := 1
    count += 2
    text := "p"
    text += "q"
    print("{point.x} {count} {text}")
}
"#,
    )
    .expect("defined hooks, numeric updates, and String append remain valid");
}
