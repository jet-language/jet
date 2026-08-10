//! D-WEBTIR1 / #123 criterion 1: web executable bodies use checked TIR only,
//! with default-deny preflight and `E-WEB-TIR-UNSUPPORTED` honest fallback.

mod common;

use std::fs;

#[test]
fn web_driver_validates_before_emit() {
    let driver = fs::read_to_string("crates/jet-driver/src/Driver/mod.rs")
        .expect("driver sources");
    assert!(
        driver.contains("validate_web_tir_support"),
        "web compile must preflight TIR support"
    );
    assert!(
        driver.contains("web emitter capability facts drifted after validation"),
        "emit-time misses must stay loud internal errors"
    );
}

#[test]
fn web_codegen_exports_tir_gate() {
    let web = fs::read_to_string("crates/jet-codegen/src/Codegen/Web.rs").expect("Web.rs");
    for needle in [
        "validate_web_tir_support",
        "web_js_handle_method_supported",
        "WebTirUnsupported",
        "lower_web_func",
        "emit_tir_js_body",
    ] {
        assert!(web.contains(needle), "missing web TIR seam `{needle}`");
    }
    for forbidden in [
        "fn js_emit_expr(expr: &Expr",
        "fn wasm_emit_expr(expr: &Expr",
        "\"undefined\".to_string()",
    ] {
        assert!(
            !web.contains(forbidden),
            "web codegen regressed to AST/default fallback: {forbidden}"
        );
    }
}
