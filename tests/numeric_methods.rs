//! D-SIGIL-ALGEBRA1=B: named numeric methods share every applicable tier.
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;
use tir_support::{assert_tiers_agree, build_and_run, have_rustc};

#[test]
fn named_methods_cover_exact_and_fixed_integer_widths() {
    let source = include_str!("../Examples/features/math/numeric_methods.jet");
    let expected = include_str!("../Examples/features/expected/math/numeric_methods.out");
    assert_tiers_agree("numeric_methods", source, expected);
    if have_rustc() {
        let (code, output) = build_and_run("numeric_methods_aot", source);
        assert_eq!(code, 0, "{output}");
        assert_eq!(output, expected);
    }
}

#[test]
fn core_sign_uses_the_source_int_contract() {
    let source = "use core.math as math\nfn run() {\n    print(math.sign(-9))\n    print(math.sign(0))\n    print(math.sign(9))\n}\n";
    let expected = "-1\n0\n1\n";
    assert_tiers_agree("core_sign_int_contract", source, expected);
    if have_rustc() {
        let (code, output) = build_and_run("core_sign_int_contract_aot", source);
        assert_eq!(code, 0, "{output}");
        assert_eq!(output, expected);
    }
}

#[test]
fn floating_contract_refusals_survive_source_routing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (fixture, code, reason) in [
        ("core_math_float_type_mismatch", "E0112", "numeric knowledge grade"),
        ("core_math_float_type_mismatch_is_nan", "E0112", "classification"),
        ("core_math_float_type_mismatch_sign", "E0112", "parameter"),
        ("core_math_float_type_mismatch_pow", "E0109", "mixing float widths"),
        ("core_math_float_type_mismatch_hypot", "E0112", "mixing float widths"),
    ] {
        let path = root.join("tests/ui").join(format!("{fixture}.jet"));
        let source = std::fs::read_to_string(&path).unwrap();
        let diagnostics = jet::compile_with_path(&source, &path.to_string_lossy())
            .expect_err("the isolated floating contract fixture must be refused");
        let rendered = jet::render_diagnostics(&format!("{fixture}.jet"), &source, &diagnostics);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "{fixture}: expected {code}\n{rendered}");
        assert!(rendered.to_lowercase().contains(reason),
            "{fixture}: expected the intended refusal, not a different error\n{rendered}");
    }
}
