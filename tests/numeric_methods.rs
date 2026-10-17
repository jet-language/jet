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
fn named_bit_methods_emit_and_run_on_u8_i64_u64() {
    let source = r#"fn run() {
    u :: U8{12}
    i :: I64{12}
    w :: U64{12}
    print("{u.bit_and(U8{10})},{u.bit_or(U8{10})},{u.bit_xor(U8{10})},{u.bit_not()},{U8{2}.pow(U8{3})},{u << 1},{u >> 1}")
    print("{i.bit_and(I64{10})},{i.bit_or(I64{10})},{i.bit_xor(I64{10})},{i.bit_not()},{I64{2}.pow(I64{3})},{i << 1},{i >> 1}")
    print("{w.bit_and(U64{10})},{w.bit_or(U64{10})},{w.bit_xor(U64{10})},{w.bit_not()},{U64{2}.pow(U64{3})},{w << 1},{w >> 1}")
}
"#;
    let expected = "8,14,6,243,8,24,6\n8,14,6,-13,8,24,6\n8,14,6,18446744073709551603,8,24,6\n";
    let rust = tir_support::compile("named_bit_methods_emit", source);
    assert!(!rust.is_empty());
    if have_rustc() {
        let (code, output) = build_and_run("named_bit_methods_aot", source);
        assert_eq!(code, 0, "{output}");
        assert_eq!(output, expected);
        let (code, output, errors) = tir_support::default_run("named_bit_methods_run", source);
        assert_eq!(code, 0, "{errors}");
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
