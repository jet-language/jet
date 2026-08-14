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
fn take(value: F32) {}

fn run() {
    value :: Float.{1.0}
    take(value)
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

    // The other knowledge planes retain their existing sema-owned gates.
    rejects(include_str!("ui/quantity_implicit_rounding.jet"), "E0127");
    rejects(include_str!("ui/typestate_wrong_state.jet"), "E0150");
    rejects(include_str!("ui/taint_sink_unsanitized.jet"), "E0721");
}
