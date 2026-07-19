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
