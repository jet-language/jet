//! D-QUAL3 (ratified 2026-06-24): unit families, `@UnitFamily(Name) { m, … }`.
//!
//! Each member mints one `@Numeric` distinct type erasing to `Float`
//! (`usd` → `Usd`), so signatures read in plain English and the compiler keeps
//! the units from mixing. This is pure sugar over the D-DIST1/D-DIST3 distinct
//! machinery: convert with `Usd.from_float(value)`, same-unit arithmetic stays in the
//! unit, `.raw()` strips it, and cross-unit mixing is E0127 (the distinct
//! same-type arithmetic rule). The family erases in codegen (I3).

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const FAMILY: &str = r#"
@UnitFamily(Currency) { usd, eur }
"#;

fn codes_of(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

fn parse_family(src: &str) -> jet::AST::UnitFamilyDef {
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty(), "lex diagnostics: {diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("unit family should parse");
    let jet::AST::Item::UnitFamily(family) = program.items.into_iter().next().unwrap() else {
        panic!("expected a unit family")
    };
    family
}

#[test]
fn scaled_and_affine_metadata_is_exact_and_normalized() {
    let family = parse_family(
        r#"@UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 2/2, offset: 54630/200)
}"#,
    );
    assert_eq!(family.base.as_ref().map(|base| base.0.as_str()), Some("kelvin"));
    let celsius = family
        .members
        .iter()
        .find(|member| member.name == "celsius")
        .unwrap();
    assert_eq!(celsius.scale.to_string(), "1");
    assert_eq!(celsius.offset.to_string(), "5463/20");
}

#[test]
fn unit_metadata_uses_arbitrary_precision_signed_ratios() {
    let src = r#"@UnitFamily(Temperature, base: kelvin) {
    kelvin
    huge(scale: -123_456_789_012_345_678_901_234_567_890/-10, offset: -0xA/-0b100)
}"#;
    let family = parse_family(src);
    let huge = family
        .members
        .iter()
        .find(|member| member.name == "huge")
        .unwrap();
    assert_eq!(huge.scale.to_string(), "12345678901234567890123456789");
    assert_eq!(huge.offset.to_string(), "5/2");
}

#[test]
fn zero_ratio_denominator_points_at_the_denominator() {
    let src = "@UnitFamily(Length, base: meter) { meter broken(scale: 1/0) }";
    let (tokens, lex) = jet::Lexer::lex(src);
    assert!(lex.is_empty(), "lex diagnostics: {lex:?}");
    let diagnostics = jet::Parser::parse(&tokens).unwrap_err();
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.what.contains("denominator is zero"))
        .expect("zero-denominator diagnostic");
    let zero = src.rfind('0').unwrap();
    assert_eq!(diagnostic.span, Some(jet::Diagnostics::Span::new(zero, zero + 1)));
}

#[test]
fn affine_family_mints_point_and_delta_types_only() {
    let family = parse_family(
        r#"@UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}"#,
    );
    let names: Vec<_> = family.distinct_defs().into_iter().map(|def| def.name).collect();
    assert_eq!(
        names,
        ["KelvinPoint", "KelvinDelta", "CelsiusPoint", "CelsiusDelta"]
    );
}

#[test]
fn scaled_family_metadata_is_public_api_identity() {
    let src = r#"pub @UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 2/2000)
}
pub fn length() -> Millimeter { return Millimeter.from_float(1.0)? }
"#;
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty());
    let program = jet::Parser::parse(&tokens).expect("scaled family should parse");
    let snapshot = jet::Publish::ApiFreeze::snapshot_from_items(&program.items, "geometry", "1.0.0");
    assert_eq!(
        snapshot.funcs[0].signature,
        "fn length() -> Millimeter{package=geometry; family=Length; base=Meter; dimension=L1T0; scale=1/1000; offset=0}"
    );
}

#[test]
fn affine_point_and_delta_have_distinct_public_identities() {
    let src = r#"pub @UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}

pub fn target() -> CelsiusPoint { return CelsiusPoint.from_float(20.0) }
pub fn tolerance() -> CelsiusDelta { return CelsiusDelta.from_float(2.0) }
"#;
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty());
    let program = jet::Parser::parse(&tokens).expect("affine family should parse");
    let snapshot = jet::Publish::ApiFreeze::snapshot_from_items(&program.items, "climate", "1.0.0");
    assert!(snapshot.funcs.iter().any(|func| func.signature ==
        "fn target() -> CelsiusPoint{package=climate; family=Temperature; base=Kelvin; dimension=L0T0H1; scale=1; offset=5463/20}"));
    assert!(snapshot.funcs.iter().any(|func| func.signature ==
        "fn tolerance() -> CelsiusDelta{package=climate; family=Temperature; base=Kelvin; dimension=L0T0H1; scale=1; offset=0}"));
}

#[test]
fn affine_point_delta_algebra_and_conversion_compile() {
    let src = r#"
@UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}
fn run() {
    freezing :: CelsiusPoint.from_float(0.0)
    step :: CelsiusDelta.from_float(5.0)
    warmer :: freezing + step
    drift :: warmer - freezing
    total :: drift + step
    absolute :: KelvinPoint.from_celsius_point_rounded(warmer, .NearestEven)
    relative :: KelvinDelta.from_celsius_delta(total) ?? panic("exact delta conversion")
    print("{(absolute.raw())} {(relative.raw())}")
}
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "Point/Delta model must be one complete algebra: {codes:?}");
}

#[test]
fn exact_same_dimension_conversion_covers_arithmetic_arguments_and_bindings() {
    let src = r#"
@UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
fn takes_millimeter(value: Millimeter) { print("{(value.raw())}") }
fn run() {
    coarse :: 3meter
    fine :: 42millimeter
    total :: coarse + fine
    takes_millimeter(3meter)
    binding: Millimeter :: 4meter
    print("{(total.raw())} {(binding.raw())}")
}
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "exact implicit conversions must share one path: {codes:?}");
}

#[test]
fn exact_concrete_coercion_uses_the_value_not_only_the_scale_denominator() {
    let src = r#"
@UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
fn takes_meter(value: Meter) { print("{(value.raw())}") }
fn run() { takes_meter(3000millimeter) }
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "3000 millimeters is exactly 3 meters: {codes:?}");
}

#[test]
fn exactness_uses_rational_math_beyond_f64_integer_precision() {
    let family = r#"
@UnitFamily(Length, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
}
"#;
    let implicit = format!(
        "{family}\nfn takes_meter(value: Meter) {{ print(value.raw()) }}\nfn run() {{ takes_meter(1almost) }}\n"
    );
    assert_eq!(codes_of(&implicit), vec!["E0127"]);

    if tir_support::have_rustc() {
        let explicit = format!(
            "{family}\nfn run() {{ value :: Meter.from_almost(1almost) ?? Meter.from_float(-1.0); print(value.raw()) }}\n"
        );
        let (code, stdout) = tir_support::build_and_run("quantity_exact_rational_edge", &explicit);
        assert_eq!(code, 0);
        assert_eq!(stdout, "-1.0\n");
    }
}

#[test]
fn quantity_generic_bound_preserves_concrete_unit_and_kind() {
    let src = r#"
@UnitFamily(Length, base: meter) { meter }
fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q { return value }
fn run() { source :: 3meter; value :: keep(^source); print("{(value.raw())}") }
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "Quantity bounds must accept a determined concrete unit: {codes:?}");
}

#[test]
fn quantity_generic_bound_rejects_wrong_dimension_and_kind() {
    let wrong_dimension = r#"
@UnitFamily(Length) { meter }
@UnitFamily(Time) { second }
fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q { return value }
fn run() { source :: 3second; keep(^source) }
"#;
    assert_eq!(codes_of(wrong_dimension), vec!["E0905"]);

    let wrong_kind = r#"
@UnitFamily(Temperature, base: kelvin) { kelvin celsius(offset: 27315/100) }
fn keep<Q: Quantity<Temperature, .Delta>>(value: ^Q) -> Q { return value }
fn run() { source :: CelsiusPoint.from_float(3.0); keep(^source) }
"#;
    assert_eq!(codes_of(wrong_kind), vec!["E0905"]);

    let undetermined = r#"
fn choose<Q: Quantity<Length, .Linear>>() {}
fn run() { choose() }
"#;
    assert_eq!(codes_of(undetermined), vec!["E0904"]);
}

#[test]
fn imported_quantity_generic_preserves_concrete_type() {
    if !tir_support::have_rustc() {
        return;
    }
    let units = r#"
pub @UnitFamily(Length) { meter }
pub fn sample() -> Meter { return 2meter }
pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q { return value }
pub fn raw_meter(value: Meter) -> Float { return value.raw() }
"#;
    let main = r#"
use "units" as units
fn run() {
    source :: units.sample()
    same :: units.keep(^source)
    print(units.raw_meter(same))
}
"#;
    let (code, stdout) = tir_support::build_and_run_multi(
        "quantity_generic_import",
        "main.jet",
        &[("units.jet", units), ("main.jet", main)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "2.0\n");
}

#[test]
fn imported_explicit_quantity_argument_checks_its_bound() {
    let dir = std::env::temp_dir().join(format!(
        "jet_quantity_imported_explicit_bound_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("units.jet"),
        "pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q { return value }\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(
        &entry,
        r#"
use "units" as units
@UnitFamily(Time) { second }
fn run() {
    source :: 1second
    bad :: units.keep<Second>(^source)
    print(bad.raw())
}
"#,
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0905"),
        "explicit imported type arguments must satisfy Quantity bounds: {diagnostics:?}"
    );
}

#[test]
fn quantity_bounds_reject_unknown_dimensions_and_kinds_at_parse_time() {
    for src in [
        "fn keep<Q: Quantity<Banana, .Linear>>(value: ^Q) -> Q { return value }",
        "fn keep<Q: Quantity<Length, .Mystery>>(value: ^Q) -> Q { return value }",
    ] {
        let (tokens, lex) = jet::Lexer::lex(src);
        assert!(lex.is_empty(), "lex diagnostics: {lex:?}");
        assert!(jet::Parser::parse(&tokens).is_err(), "invalid Quantity bound parsed: {src}");
    }
}

#[test]
fn quantity_generic_bounds_are_frozen_into_public_api_identity() {
    let parse = |src: &str| {
        let (tokens, diagnostics) = jet::Lexer::lex(src);
        assert!(diagnostics.is_empty());
        jet::Parser::parse(&tokens).expect("public Quantity generic should parse")
    };
    let length = parse(
        "pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q { return value }",
    );
    let time = parse(
        "pub fn keep<Q: Quantity<Time, .Linear>>(value: ^Q) -> Q { return value }",
    );
    let length = jet::Publish::ApiFreeze::snapshot_from_items(&length.items, "units", "1.0.0");
    let time = jet::Publish::ApiFreeze::snapshot_from_items(&time.items, "units", "1.0.0");
    assert_eq!(
        length.funcs[0].signature,
        "fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) -> Q"
    );
    assert_ne!(length.funcs[0].signature, time.funcs[0].signature);
    let old = vec![jet::Publish::ApiItem {
        kind: "fn".into(),
        name: "keep".into(),
        signature: length.funcs[0].signature.clone(),
    }];
    let new = vec![jet::Publish::ApiItem {
        kind: "fn".into(),
        name: "keep".into(),
        signature: time.funcs[0].signature.clone(),
    }];
    assert_eq!(jet::Publish::diff_public_api(&old, &new).len(), 1);
}

#[test]
fn explicit_units_policy_requires_destination_owned_conversion() {
    let implicit = r#"
@UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
@Policy(explicit_units)
fn run() {
    total :: 1meter + 1millimeter
    print("{(total.raw())}")
}
"#;
    assert_eq!(codes_of(implicit), vec!["E0127"]);

    let explicit = r#"
@UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
@Policy(explicit_units)
fn run() {
    converted :: Meter.from_millimeter(1000millimeter) ?? panic("exact conversion")
    total :: 1meter + converted
    print("{(total.raw())}")
}
"#;
    assert!(codes_of(explicit).is_empty());

    let module_scoped = r#"
@Policy(explicit_units)
@UnitFamily(Length, base: meter) { meter millimeter(scale: 1/1000) }
fn run() { total :: 1meter + 1millimeter; print(total.raw()) }
"#;
    assert_eq!(codes_of(module_scoped), vec!["E0127"]);

    let block_scoped = r#"
@UnitFamily(Length, base: meter) { meter millimeter(scale: 1/1000) }
fn run() {
    @Policy(explicit_units) {
        total :: 1meter + 1millimeter
        print(total.raw())
    }
}
"#;
    assert_eq!(codes_of(block_scoped), vec!["E0127"]);
}

#[test]
fn implicit_unit_conversion_rejects_rounding_and_overflow_boundaries() {
    let rounding = r#"
@UnitFamily(Length, base: meter) {
    meter
    thirdish(scale: 2/3)
}
fn run() { value :: 1meter + 1thirdish; print("{(value.raw())}") }
"#;
    assert_eq!(codes_of(rounding), vec!["E0127"]);

    let overflow = format!(
        "@UnitFamily(Length, base: meter) {{ meter giant(scale: {}) }}\nfn run() {{ value :: 1giant + 1meter; print(\"{{(value.raw())}}\") }}",
        "9".repeat(400)
    );
    assert_eq!(codes_of(&overflow), vec!["E0127"]);

    let explicit_overflow = format!(
        "@UnitFamily(Length, base: meter) {{ meter giant(scale: {}) }}\nfn run() {{ value :: Meter.from_giant(1giant); print(\"{{(value.raw())}}\") }}",
        "9".repeat(400)
    );
    assert_eq!(codes_of(&explicit_overflow), vec!["E0127"]);
}

#[test]
fn explicit_unit_conversion_is_fallible_and_rounded_spelling_is_real() {
    if !tir_support::have_rustc() {
        return;
    }
    let src = r#"
@UnitFamily(Length, base: meter) {
    meter
    millimeter(scale: 1/1000)
    thirdish(scale: 2/3)
}
fn run() {
    exact :: Meter.from_millimeter(3000millimeter) ?? panic("exact conversion failed")
    inexact :: Meter.from_thirdish(1thirdish) ?? Meter.from_float(-1.0)
    rounded :: Meter.from_thirdish_rounded(1thirdish, .NearestEven)
    print("{(exact.raw())} {(inexact.raw())} {(rounded.raw())}")
}
"#;
    let generated = jet::compile(src).expect("explicit unit conversions should compile").rust;
    assert!(
        !generated.contains("fn user_from_"),
        "unit conversion behavior belongs to TIR, not generated destination methods"
    );
    let (code, stdout) = tir_support::build_and_run("quantity_explicit_exact_rounded", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3.0 -1.0 1.0\n");

    let unchecked = r#"
@UnitFamily(Length, base: meter) {
    meter
    thirdish(scale: 2/3)
}
fn run() {
    converted :: Meter.from_thirdish(1thirdish)
    print(converted.raw())
}
"#;
    assert!(
        !codes_of(unchecked).is_empty(),
        "an exact conversion result must be handled before use"
    );
}

#[test]
fn physical_unit_codable_round_trips_as_its_concrete_type() {
    if !tir_support::have_rustc() {
        return;
    }
    let src = r#"
use core.encoding.json as json
@UnitFamily(Length) { meter }
fn run() {
    value :: 3meter
    wire :: json.to_string(value)
    back :: json.decode<Meter>(wire) ?? panic("decode")
    print(wire)
    print(back.raw())
}
"#;
    let (code, stdout) = tir_support::build_and_run("quantity_codable", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3.0\n3.0\n");
}

/// A member name is usable as a distinct type in a signature; construction,
/// same-unit arithmetic and `.raw()` all compile cleanly.
#[test]
fn same_unit_arithmetic_compiles() {
    let src = format!(
        "{}\nfn add(a: Usd, b: Usd) -> Usd {{ return a + b }}\nfn run() {{ t :: add(Usd.from_float(1.0), Usd.from_float(2.0)); print(\"{{(t.raw())}}\") }}\n",
        FAMILY
    );
    let codes = codes_of(&src);
    assert!(codes.is_empty(), "expected clean compile, got {:?}", codes);
}

/// Mixing two members of the family is rejected — E0127 (different distinct
/// types in arithmetic), the same rule any two distinct types follow.
#[test]
fn cross_unit_mix_is_e0127() {
    let src = format!(
        "{}\nfn run() {{ bad :: Usd.from_float(1.0) + Eur.from_float(2.0); print(\"{{(bad.raw())}}\") }}\n",
        FAMILY
    );
    let codes = codes_of(&src);
    assert!(
        codes.contains(&"E0127".to_string()),
        "expected E0127 for cross-unit mixing, got {:?}",
        codes
    );
}

/// The base type does not implicitly coerce into a unit member (D-DIST3).
#[test]
fn bare_float_does_not_coerce_into_unit() {
    let src = format!(
        "{}\nfn take(p: Usd) {{ print(\"{{(p.raw())}}\") }}\nfn run() {{ take(9.99) }}\n",
        FAMILY
    );
    let codes = codes_of(&src);
    assert!(
        !codes.is_empty(),
        "passing a bare Float where Usd is expected must be rejected, got clean"
    );
}

/// Member names PascalCase to their type names: `m_per_s` → `MPerS`.
#[test]
fn multi_word_member_pascal_cases() {
    let src = r#"
@UnitFamily(Speed) { m_per_s }
fn run() {
    v :: MPerS.from_float(3.0)
    print("{(v.raw())}")
}
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "expected clean compile, got {:?}", codes);
}

/// The family erases in codegen: each member lowers to a `#[repr(transparent)]`
/// distinct newtype over the base, no marker artifact, no `unsafe` (I1/I3).
#[test]
fn family_erases_in_codegen() {
    let src = format!(
        "{}\nfn run() {{ t :: Usd.from_float(1.0); print(\"{{(t.raw())}}\") }}\n",
        FAMILY
    );
    let out = jet::compile(&src).expect("should compile");
    assert!(
        !common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
        "I1: no unsafe in generated code"
    );
    assert!(
        !out.rust.contains("UnitFamily"),
        "the family marker must erase, found UnitFamily in output"
    );
    assert!(
        out.rust.contains("struct user_Usd"),
        "Usd should lower to a distinct newtype"
    );
    assert!(
        out.rust.contains("struct user_Eur"),
        "Eur should lower to a distinct newtype"
    );
    assert!(
        out.rust.contains("#[repr(transparent)]"),
        "members erase to a transparent newtype over the base"
    );
}

/// D-SHAPE-QUANTITY1=A: physical families participate in normalized
/// dimensional algebra while their runtime representation stays numeric.
#[test]
fn physical_dimensions_derive_before_codegen_and_erase_at_runtime() {
    let src = r#"
@UnitFamily(Length) { meter }
@UnitFamily(Time) { second }

fn run() {
    distance :: 12meter
    elapsed :: 3second
    speed :: distance / elapsed
    area :: distance * distance
    recovered :: speed * elapsed
    print("ok")
}
"#;
    let out = jet::compile(src).expect("dimensionally valid program should compile");
    assert!(out.rust.contains("user_Meter"));
    assert!(out.rust.contains("user_Second"));
    assert!(out.rust.contains(".0 /"), "unit division must erase to base arithmetic");
    assert!(out.rust.contains(".0 *"), "unit multiplication must erase to base arithmetic");
    assert!(!out.rust.contains("Quantity<"), "dimension facts must not reach emitted Rust");
}

#[test]
fn physical_dimension_mismatch_is_rejected_in_sema() {
    let src = r#"
@UnitFamily(Length) { meter }
@UnitFamily(Time) { second }
fn run() { bad :: 1meter + 1second }
"#;
    let codes = codes_of(src);
    assert_eq!(codes, vec!["E0359"], "expected one dimension mismatch, got {codes:?}");
}

#[test]
fn physical_value_cannot_compare_with_scalar() {
    let src = r#"
@UnitFamily(Length) { meter }
fn run() { bad :: 1meter < 1.0 }
"#;
    assert_eq!(codes_of(src), vec!["E0359"]);
}

#[test]
fn dimension_exponent_limit_is_a_sema_error_not_a_panic() {
    let mut src = String::from("@UnitFamily(Length) { meter }\nfn run() {\n    q0 :: 1meter\n");
    for exponent in 1..=31 {
        src.push_str(&format!(
            "    q{exponent} :: q{} * q{}\n",
            exponent - 1,
            exponent - 1
        ));
    }
    src.push_str("}\n");
    let result = std::panic::catch_unwind(|| jet::compile(&src));
    let diagnostics = result
        .expect("checked dimension overflow must not panic")
        .expect_err("2^31 Length exponent must be rejected in sema");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0359"));
}

#[test]
fn imported_same_leaf_units_keep_distinct_dimensions() {
    let length = r#"
@UnitFamily(Length) { unit }
pub fn sample() -> [[String: [Unit]]] { return [["values": [1unit]]] }
pub fn first(groups: [[String: [Unit]]]) -> Unit { return ~groups[0]["values"][0] }
"#;
    let time = r#"
@UnitFamily(Time) { unit }
pub fn sample() -> [[String: [Unit]]] { return [["values": [1unit]]] }
pub fn first(groups: [[String: [Unit]]]) -> Unit { return ~groups[0]["values"][0] }
"#;
    let good = r#"
use "length" as length
use "time" as time
fn run() {
    distance :: length.first(length.sample()) + length.first(length.sample())
    elapsed :: time.first(time.sample()) + time.first(time.sample())
    print("ok")
}
"#;
    if tir_support::have_rustc() {
        let (code, stdout) = tir_support::build_and_run_multi(
            "quantity_composite_same_leaf",
            "main.jet",
            &[("length.jet", length), ("time.jet", time), ("main.jet", good)],
        );
        assert_eq!(code, 0);
        assert_eq!(stdout, "ok\n");
    }

    let dir = std::env::temp_dir().join(format!("jet_quantity_collision_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("length.jet"), length).unwrap();
    std::fs::write(dir.join("time.jet"), time).unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, r#"
use "length" as length
use "time" as time
fn run() { bad :: length.first(length.sample()) + time.first(time.sample()) }
"#).unwrap();
    let src = std::fs::read_to_string(&entry).unwrap();
    let result = jet::compile_with_path(&src, entry.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    let codes: Vec<_> = result.unwrap_err().into_iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, vec!["E0359"]);
}

#[test]
fn same_named_unit_families_from_different_packages_do_not_convert() {
    let dir = std::env::temp_dir().join(format!(
        "jet_quantity_package_identity_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let unit = r#"
pub @UnitFamily(Length, base: meter) { meter millimeter(scale: 1/1000) }
pub fn sample() -> Meter { return 1meter }
"#;
    std::fs::write(dir.join("left.jet"), unit).unwrap();
    std::fs::write(dir.join("right.jet"), unit).unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(
        &entry,
        r#"
use "left" as left
use "right" as right
fn run() { bad :: left.sample() + right.sample(); print(bad.raw()) }
"#,
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    for module in &mut bundle.modules {
        if matches!(module.alias.as_str(), "left" | "right") {
            module.path = dir
                .parent()
                .unwrap()
                .join(format!("foreign-{}/module.jet", module.alias));
        }
    }
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code.as_str(), "E0109" | "E0127" | "E0359")),
        "package-owned families must not unify: {diagnostics:?}"
    );
}

#[test]
fn currency_keeps_nominal_arithmetic_behavior() {
    let src = format!(
        "{}\nfn run() {{ total :: 2usd * 3usd; print(\"{{(total.raw())}}\") }}\n",
        FAMILY
    );
    let codes = codes_of(&src);
    assert!(codes.is_empty(), "Currency is outside physical dimension math: {codes:?}");
}

#[test]
fn physical_dimensions_cross_file_boundaries_canonically() {
    let dir = std::env::temp_dir().join(format!(
        "jet_quantity_packages_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("units.jet"),
        r#"
@UnitFamily(Length) { meter }
@UnitFamily(Time) { second }
pub fn distance() -> Meter { return 12meter }
pub fn elapsed() -> Second { return 3second }
"#,
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(
        &entry,
        r#"
use "units" as units
fn run() {
    speed :: units.distance() / units.elapsed()
    recovered :: speed * units.elapsed()
    print("ok")
}
"#,
    )
    .unwrap();
    let src = std::fs::read_to_string(&entry).unwrap();
    let result = jet::compile_with_path(&src, entry.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    let out = result.expect("imported physical units should share canonical dimensions");
    assert!(!out.rust.contains("Quantity<"));
}
