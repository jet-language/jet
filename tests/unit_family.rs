//! D-QUAL3 (ratified 2026-06-24): unit families, `#UnitFamily(Name) { m, … }`.
//!
//! Each member mints one `#Numeric` distinct type erasing to `Float`
//! (`usd` → `Usd`), so signatures read in plain English and the compiler keeps
//! the units from mixing. This is pure sugar over the D-DIST1/D-DIST3 distinct
//! machinery: convert with `Usd.from_float(value)`, same-unit arithmetic stays in the
//! unit, `.raw()` strips it, and cross-unit mixing is E0127 (the distinct
//! same-type arithmetic rule). The family erases in codegen (I3).

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const FAMILY: &str = r#"
#UnitFamily(Currency) { usd, eur }
"#;

fn codes_of(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

fn check_codes_of(src: &str) -> Vec<String> {
    let dir = common::unique_tmp("jet_quantity_check");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, src).unwrap();
    jet::Driver::check_file(path.to_str().unwrap(), None, false)
        .0
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
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
fn open_base_and_derived_dimension_claims_parse() {
    let base = parse_family("#UnitFamily(Mass, dimension, base: kilogram) { kilogram }");
    assert!(matches!(
        base.dimension,
        Some(jet::AST::UnitDimensionDecl::Base(_))
    ));

    let derived = parse_family(
        "#UnitFamily(Force, dimension: Mass * Length / Time / Time, base: newton) { newton }",
    );
    assert!(matches!(
        derived.dimension,
        Some(jet::AST::UnitDimensionDecl::Derived(_))
    ));
}

#[test]
fn derived_dimension_requires_one_scale_one_anchor() {
    let src = "#UnitFamily(Energy, dimension: Force * Length) { joule }";
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty(), "lex diagnostics: {diagnostics:?}");
    let diagnostics = jet::Parser::parse(&tokens).expect_err("a derived dimension needs `base:`");
    assert_eq!(
        diagnostics.iter().map(|diagnostic| diagnostic.code.as_str()).collect::<Vec<_>>(),
        ["E0003"]
    );
    assert_eq!(
        diagnostics[0].fix,
        "add `base: member_name` and keep that member's scale at 1 with offset 0"
    );
}

#[test]
fn scale_provenance_parses_without_erasing_source_truth() {
    let angle = parse_family(
        "#UnitFamily(Angle, dimension, base: radian) { radian degree(scale: pi / 180) }",
    );
    assert!(matches!(
        angle.members[1].scale_provenance,
        jet::AST::UnitScaleProvenance::SymbolicPi { .. }
    ));

    let mass = parse_family(
        r#"#UnitFamily(Mass, dimension, base: kilogram) {
    kilogram
    dalton(scale: measured(1.66053906892e-27, uncertainty: 0.00000000052e-27, source: "BIPM-2026/CODATA-2022"))
}"#,
    );
    assert!(matches!(
        mass.members[1].scale_provenance,
        jet::AST::UnitScaleProvenance::Measured { .. }
    ));
}

#[test]
fn symbolic_and_measured_standard_scales_keep_one_explicit_boundary() {
    let symbolic = r#"
fn accept(value: Radian) {}
fn run() { accept(1degree) }
"#;
    assert!(
        codes_of(symbolic).is_empty(),
        "symbolic pi conversions should use the ordinary conversion path"
    );

    let measured = r#"
fn run() {
    mass :: Kilogram.from_dalton_rounded(1dalton, .NearestEven, digits: 30) ?? panic("rounded measured conversion")
    print(mass.raw())
}
"#;
    assert!(
        codes_of(measured).is_empty(),
        "a written rounded conversion is the audit boundary for measured scales"
    );
}

#[test]
fn poundforce_uses_the_exact_defined_ratio_across_execution_tiers() {
    let src = r#"
fn run() {
    force :: Newton.from_poundforce(10000000000000poundforce) ?? panic("defined force conversion")
    print(force.raw())
}
"#;
    assert!(
        include_str!("../crates/jet-codegen/src/Prelude/Units.jet")
            .contains("poundforce(scale: 44482216152605/10000000000000)"),
        "the standard unit declaration lost the exact defined pound-force ratio"
    );
    jet::compile(src).expect("standard force conversion should compile");

    let (code, stdout, stderr) =
        tir_support::build_and_run_full("jet_unit_family", "standard_force_ratio", src);
    assert_eq!((code, stdout.as_str()), (0, "44482216152605.0\n"), "{stderr}");

    let dir = common::unique_tmp("standard_force_ratio_tiers");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, src).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("standard force conversion must lower through shared TIR");
    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_program(
        &program,
        &bundle.project_root,
        &mut sink,
        std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        true,
    )
    .expect("standard force conversion must run in the evaluator");
    assert_eq!(sink.stdout, "44482216152605.0\n");

    if jet_jit::cranelift_host_supported() {
        use jet::JitBackend::{JitBackend, RunOutcome};
        jet_jit::reset_jit_trace_for_test();
        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, "44482216152605.0\n"),
            RunOutcome::Problems(diagnostics) => {
                panic!("JIT rejected standard force conversion: {diagnostics:?}")
            }
        }
        assert!(jet_jit::jit_executed_for_test());
        assert!(
            !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
            "standard force conversion must stay on resident JIT"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn user_dimensions_compose_and_named_derived_units_share_the_structure() {
    let src = r#"
#UnitFamily(Mass, dimension, base: kilogram) { kilogram }
#UnitFamily(Length, dimension, base: meter) { meter }
#UnitFamily(Time, dimension, base: second) { second }
#UnitFamily(Force, dimension: Mass * Length / Time / Time, base: newton) { newton }

fn accept(force: Newton) { print("{force.raw()}") }
fn keep<Q: Quantity<Force, .Linear>>(value: ^Q) => Q { return value }
fn run() {
    momentum :: 4kilogram * 3meter
    acceleration_step :: momentum / 2second
    force :: acceleration_step / 2second
    named :: 1newton
    keep(^named)
    keep(^force)
    accept(force)
}
"#;
    let (code, stdout) = tir_support::build_and_run("open_dimensions", src);
    assert_eq!((code, stdout.as_str()), (0, "3.0\n"));
}

#[test]
fn user_dimensions_keep_interpreter_and_resident_jit_parity() {
    let src = r#"
#UnitFamily(Mass, dimension, base: kilogram) { kilogram }
#UnitFamily(Length, dimension, base: meter) { meter }
#UnitFamily(Time, dimension, base: second) { second }
#UnitFamily(Force, dimension: Mass * Length / Time / Time, base: newton) { newton }

fn accept(force: Newton) { print("{force.raw()}") }
fn keep<Q: Quantity<Force, .Linear>>(value: ^Q) => Q { return value }
fn run() {
    momentum :: 4kilogram * 3meter
    acceleration_step :: momentum / 2second
    force :: acceleration_step / 2second
    keep(^force)
    accept(force)
}
"#;
    let dir = common::unique_tmp("open_dimension_tiers");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, src).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("open dimensions must lower through shared TIR");
    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_program(
        &program,
        &bundle.project_root,
        &mut sink,
        std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        true,
    )
    .expect("open dimensions must run in the evaluator");
    assert_eq!(sink.stdout, "3.0\n");

    if jet_jit::cranelift_host_supported() {
        use jet::JitBackend::{JitBackend, RunOutcome};
        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, "3.0\n"),
            RunOutcome::Problems(diagnostics) => {
                panic!("JIT rejected open dimensions: {diagnostics:?}")
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn scaled_and_affine_metadata_is_exact_and_normalized() {
    let family = parse_family(
        r#"#UnitFamily(Temperature, dimension, base: kelvin) {
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
    let src = r#"#UnitFamily(Temperature, dimension, base: kelvin) {
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
    let src = "#UnitFamily(Length, dimension, base: meter) { meter broken(scale: 1/0) }";
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
        r#"#UnitFamily(Temperature, dimension, base: kelvin) {
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
    let src = r#"pub #UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 2/2000)
}
pub fn length() => Millimeter { return Millimeter.from_float(1.0)? }
"#;
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty());
    let program = jet::Parser::parse(&tokens).expect("scaled family should parse");
    let snapshot = jet::Publish::ApiFreeze::snapshot_from_items(&program.items, "geometry", "1.0.0");
    assert_eq!(
        snapshot.funcs[0].signature,
        "fn length() => Millimeter{package=geometry; family=Length; base=Meter; dimension=geometry%3A%3ALength:1; scale=1/1000; provenance=Rational; offset=0}"
    );
}

#[test]
fn affine_point_and_delta_have_distinct_public_identities() {
    let src = r#"pub #UnitFamily(Temperature, dimension, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}

pub fn target() => CelsiusPoint { return CelsiusPoint.from_float(20.0) }
pub fn tolerance() => CelsiusDelta { return CelsiusDelta.from_float(2.0) }
"#;
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(diagnostics.is_empty());
    let program = jet::Parser::parse(&tokens).expect("affine family should parse");
    let snapshot = jet::Publish::ApiFreeze::snapshot_from_items(&program.items, "climate", "1.0.0");
    assert!(snapshot.funcs.iter().any(|func| func.signature ==
        "fn target() => CelsiusPoint{package=climate; family=Temperature; base=Kelvin; dimension=climate%3A%3ATemperature:1; scale=1; provenance=Rational; offset=5463/20}"));
    assert!(snapshot.funcs.iter().any(|func| func.signature ==
        "fn tolerance() => CelsiusDelta{package=climate; family=Temperature; base=Kelvin; dimension=climate%3A%3ATemperature:1; scale=1; provenance=Rational; offset=0}"));
}

#[test]
fn affine_point_delta_algebra_and_conversion_compile() {
    let src = r#"
#UnitFamily(Temperature, dimension, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}
fn run() {
    freezing :: CelsiusPoint.from_float(0.0)
    step :: CelsiusDelta.from_float(5.0)
    warmer :: freezing + step
    drift :: warmer - freezing
    total :: drift + step
    absolute :: KelvinPoint.from_celsius_point_rounded(warmer, .NearestEven, digits: 0) ?? panic("rounded point conversion")
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
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
fn takes_millimeter(value: Millimeter) { print("{(value.raw())}") }
fn run() {
    coarse :: 3meter
    fine :: 42millimeter
    total :: coarse + fine
    takes_millimeter(3meter)
    binding :: Millimeter.{ 4meter }
    print("{(total.raw())} {(binding.raw())}")
}
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "exact implicit conversions must share one path: {codes:?}");
}

#[test]
fn exact_concrete_coercion_uses_the_value_not_only_the_scale_denominator() {
    let src = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
fn takes_meter(value: Meter) { print("{(value.raw())}") }
fn run() { takes_meter(3000millimeter) }
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "3000 millimeters is exactly 3 meters: {codes:?}");
    let generated = jet::compile(src).expect("implicit exact conversion should compile").rust;
    assert!(
        generated.contains("Meter(match jet_unit_conversion_exact("),
        "implicit conversion must lower through the shared TIR UnitConvert path"
    );

    let family = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    double(scale: 2)
}
fn takes_meter(value: Meter) { print(value.raw()) }
"#;
    for value in ["0.25", "1.7976931348623157e308"] {
        let src = format!(
            "{family}\nfn relay(value: Double) {{ takes_meter(value) }}\nfn run() {{ relay(Double.from_float({value})) }}\n"
        );
        assert_eq!(
            check_codes_of(&src),
            vec!["E0127"],
            "unknown scale-2 value {value} must be rejected before runtime"
        );
    }

    let direct_negative = format!(
        "{family}\nfn run() {{ takes_meter(Double.from_float(-1.0)) }}\n"
    );
    assert!(
        check_codes_of(&direct_negative).is_empty(),
        "direct negative literal has an exact scale-2 conversion"
    );

    let bound_negative = format!(
        "{family}\nfn run() {{ value :: Double.from_float(-2.0)\n takes_meter(value) }}\n"
    );
    assert!(
        check_codes_of(&bound_negative).is_empty(),
        "immutable negative literal binding preserves its exact value"
    );

    let inexact_negative = format!(
        "{family}\nfn run() {{ takes_meter(Double.from_float(-0.25)) }}\n"
    );
    assert_eq!(
        check_codes_of(&inexact_negative),
        vec!["E0127"],
        "negative literal proof must still reject an inexact conversion"
    );

    let identity = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    alias(scale: 1)
}
fn takes_meter(value: Meter) { print(value.raw()) }
fn relay(value: Alias) { takes_meter(value) }
fn run() { relay(Alias.from_float(0.25)) }
"#;
    assert!(
        check_codes_of(identity).is_empty(),
        "identity conversion is exact over the complete Float domain"
    );
}

#[test]
fn exactness_uses_rational_math_beyond_f64_integer_precision() {
    let family = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
}
"#;
    let implicit = format!(
        "{family}\nfn takes_meter(value: Meter) {{ print(value.raw()) }}\nfn run() {{ takes_meter(1almost) }}\n"
    );
    assert_eq!(codes_of(&implicit), vec!["E0127"]);

    let unrepresentable_implicit = format!(
        "{family}\nfn takes_meter(value: Meter) {{ print(value.raw()) }}\nfn run() {{ takes_meter(Almost.from_float(9007199254740992.0)) }}\n"
    );
    assert_eq!(codes_of(&unrepresentable_implicit), vec!["E0127"]);

    if tir_support::have_rustc() {
        let explicit = format!(
            "{family}\nfn run() {{ value :: Meter.from_almost(1almost) ?? Meter.from_float(-1.0); print(value.raw()) }}\n"
        );
        let (code, stdout) = tir_support::build_and_run("quantity_exact_rational_edge", &explicit);
        assert_eq!(code, 0);
        assert_eq!(stdout, "-1.0\n");

        let rounding = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    almost(scale: 9007199254740993/9007199254740992)
    half(scale: 1/2)
    above_half(scale: 9007199254740993/18014398509481984)
    three_halves(scale: 3/2)
}
#UnitFamily(Temperature, dimension, base: kelvin) {
    kelvin
    tie_offset(scale: 1, offset: 1/2)
    above_offset(scale: 1, offset: 9007199254740993/18014398509481984)
    below_offset(scale: 1, offset: -9007199254740993/18014398509481984)
}
fn run() {
    exact :: Meter.from_almost(Almost.from_float(9007199254740992.0)) ?? Meter.from_float(-1.0)
    tie :: Meter.from_half_rounded(Half.from_float(1.0), .NearestEven, digits: 0) ?? panic("tie")
    above :: Meter.from_above_half_rounded(AboveHalf.from_float(1.0), .NearestEven, digits: 0) ?? panic("above")
    negative :: Meter.from_three_halves_rounded(ThreeHalves.from_float(-1.0), .NearestEven, digits: 0) ?? panic("negative")
    affine_tie :: KelvinPoint.from_tie_offset_point_rounded(TieOffsetPoint.from_float(0.0), .NearestEven, digits: 0) ?? panic("affine tie")
    affine_above :: KelvinPoint.from_above_offset_point_rounded(AboveOffsetPoint.from_float(0.0), .NearestEven, digits: 0) ?? panic("affine above")
    affine_below :: KelvinPoint.from_below_offset_point_rounded(BelowOffsetPoint.from_float(0.0), .NearestEven, digits: 0) ?? panic("affine below")
    print("{(exact.raw())} {(tie.raw())} {(above.raw())} {(negative.raw())} {(affine_tie.raw())} {(affine_above.raw())} {(affine_below.raw())}")
}
"#;
        let (code, stdout) =
            tir_support::build_and_run("quantity_exact_rational_rounding_edges", rounding);
        assert_eq!(code, 0);
        assert_eq!(stdout, "-1.0 0.0 1.0 -2.0 0.0 1.0 -1.0\n");

        let overflow = r#"
#UnitFamily(Length, dimension, base: meter) { meter double(scale: 2) }
fn run() {
    source :: Double.from_float(1.7976931348623157e308)
    value :: Meter.from_double_rounded(source, .NearestEven, digits: 0) ?? Meter.from_float(-1.0)
    print(value.raw())
}
"#;
        let (code, stdout, stderr) = tir_support::build_and_run_full(
            "quantity_unit_conversion",
            "rounded_overflow",
            overflow,
        );
        assert_eq!(code, 0);
        assert_eq!(stdout, "-1.0\n");
        assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
    }

    assert_eq!(
        jet_foundation::jet_unit_conversion_rounded(
            f64::MAX,
            "2",
            "1",
            "0",
            "1",
            jet_foundation::UnitRoundingMode::NearestEven,
            0,
        ),
        Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE),
    );
    assert_eq!(
        jet_foundation::jet_unit_conversion_rounded(
            f64::INFINITY,
            "1",
            "1",
            "0",
            "1",
            jet_foundation::UnitRoundingMode::NearestEven,
            0,
        ),
        Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE),
    );
}

#[test]
fn rounded_conversion_honors_mode_digits_affinity_and_fallibility() {
    if !tir_support::have_rustc() {
        return;
    }
    let src = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    half(scale: 1/2)
    eighth(scale: 1/8)
    near_quarter(scale: 249/1000)
    near_three_quarters(scale: 751/1000)
    double(scale: 2)
}
#UnitFamily(Temperature, dimension, base: kelvin) {
    kelvin
    shifted(scale: 1, offset: 249/1000)
}
fn run() {
    positive :: Half.from_float(5.0)
    negative :: Half.from_float(-5.0)
    positive_odd_tie :: Half.from_float(7.0)
    negative_odd_tie :: Half.from_float(-7.0)
    toward_zero_positive :: Meter.from_half_rounded(positive, .TowardZero, digits: 0) ?? panic("toward zero positive")
    floor_positive :: Meter.from_half_rounded(positive, .Floor, digits: 0) ?? panic("floor positive")
    ceiling_positive :: Meter.from_half_rounded(positive, .Ceiling, digits: 0) ?? panic("ceiling positive")
    nearest_positive :: Meter.from_half_rounded(positive, .NearestEven, digits: 0) ?? panic("nearest positive")
    toward_zero_negative :: Meter.from_half_rounded(negative, .TowardZero, digits: 0) ?? panic("toward zero negative")
    floor_negative :: Meter.from_half_rounded(negative, .Floor, digits: 0) ?? panic("floor negative")
    ceiling_negative :: Meter.from_half_rounded(negative, .Ceiling, digits: 0) ?? panic("ceiling negative")
    nearest_negative :: Meter.from_half_rounded(negative, .NearestEven, digits: 0) ?? panic("nearest negative")
    nearest_positive_odd :: Meter.from_half_rounded(positive_odd_tie, .NearestEven, digits: 0) ?? panic("nearest positive odd")
    nearest_negative_odd :: Meter.from_half_rounded(negative_odd_tie, .NearestEven, digits: 0) ?? panic("nearest negative odd")
    nearest_quarter :: Meter.from_near_quarter_rounded(1near_quarter, .NearestEven, digits: 2) ?? panic("nearest quarter")
    nearest_three_quarters :: Meter.from_near_three_quarters_rounded(1near_three_quarters, .NearestEven, digits: 2) ?? panic("nearest three quarters")
    unrepresentable_decimal :: Meter.from_eighth_rounded(1eighth, .NearestEven, digits: 2) ?? Meter.from_float(-2.0)
    point :: KelvinPoint.from_shifted_point_rounded(ShiftedPoint.from_float(0.0), .Ceiling, digits: 2) ?? panic("point")
    delta :: KelvinDelta.from_shifted_delta_rounded(ShiftedDelta.from_float(0.0), .Ceiling, digits: 2) ?? panic("delta")
    overflow :: Meter.from_double_rounded(Double.from_float(1.7976931348623157e308), .NearestEven, digits: 0) ?? Meter.from_float(-1.0)
    print("{(toward_zero_positive.raw())} {(floor_positive.raw())} {(ceiling_positive.raw())} {(nearest_positive.raw())} {(toward_zero_negative.raw())} {(floor_negative.raw())} {(ceiling_negative.raw())} {(nearest_negative.raw())} {(nearest_positive_odd.raw())} {(nearest_negative_odd.raw())} {(nearest_quarter.raw())} {(nearest_three_quarters.raw())} {(unrepresentable_decimal.raw())} {(point.raw())} {(delta.raw())} {(overflow.raw())}")
}
"#;
    let (code, stdout) = tir_support::build_and_run("quantity_rounded_contract", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "2.0 2.0 3.0 2.0 -2.0 -3.0 -2.0 -2.0 4.0 -4.0 0.25 0.75 -2.0 0.25 0.0 -1.0\n"
    );

    let negative_digits = r#"
#UnitFamily(Length, dimension, base: meter) { meter half(scale: 1/2) }
fn run() => Void ? {
    digits :: -1
    Meter.from_half_rounded(1half, .NearestEven, digits: digits)?
}
"#;
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "quantity_unit_conversion",
        "rounded_negative_digits",
        negative_digits,
    );
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(
        stderr.ends_with("rounded unit conversion needs nonnegative digits\n"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn rounded_conversion_rejects_float_precision_loss_and_bounds_huge_digits() {
    use jet_foundation::UnitRoundingMode::{Ceiling, Floor, NearestEven, TowardZero};

    for scale in ["9007199254740993", "-9007199254740993"] {
        assert_eq!(
            jet_foundation::jet_unit_conversion_rounded(
                1.0,
                scale,
                "1",
                "0",
                "1",
                NearestEven,
                0,
            ),
            Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE),
            "rounded conversion must not add Float precision loss"
        );
    }

    for mode in [TowardZero, Floor, Ceiling, NearestEven] {
        for value in [0.5, -0.5, f64::from_bits(1), -f64::from_bits(1), f64::MAX] {
            assert_eq!(
                jet_foundation::jet_unit_conversion_rounded(
                    value,
                    "1",
                    "1",
                    "0",
                    "1",
                    mode,
                    i64::MAX,
                ),
                Ok(value),
                "huge decimal precision must preserve exact finite Float {value:?} in {mode:?}"
            );
        }
        assert_eq!(
            jet_foundation::jet_unit_conversion_rounded(
                1.0,
                "1",
                "3",
                "0",
                "1",
                mode,
                i64::MAX,
            ),
            Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE),
            "huge precision must reject a rounded rational Float cannot represent"
        );
    }

    assert_eq!(
        jet_foundation::jet_unit_conversion_rounded(
            f64::from_bits(1),
            "1",
            "1",
            "0",
            "1",
            NearestEven,
            1074,
        ),
        Ok(f64::from_bits(1)),
        "the bounded path must retain the smallest subnormal"
    );

    let denominator = format!("1{}", "0".repeat(2001));
    let just_above_one = format!("1{}1", "0".repeat(2000));
    let halfway_above_one = format!("1{}5", "0".repeat(2000));
    let just_below_negative_one = format!("-{just_above_one}");
    let halfway_below_negative_one = format!("-{halfway_above_one}");
    for (scale, toward_zero, floor, ceiling, nearest) in [
        (just_above_one.as_str(), Ok(1.0), Ok(1.0), Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE), Ok(1.0)),
        (just_below_negative_one.as_str(), Ok(-1.0), Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE), Ok(-1.0), Ok(-1.0)),
        (halfway_above_one.as_str(), Ok(1.0), Ok(1.0), Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE), Ok(1.0)),
        (halfway_below_negative_one.as_str(), Ok(-1.0), Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE), Ok(-1.0), Ok(-1.0)),
    ] {
        for (mode, expected) in [
            (TowardZero, toward_zero),
            (Floor, floor),
            (Ceiling, ceiling),
            (NearestEven, nearest),
        ] {
            assert_eq!(
                jet_foundation::jet_unit_conversion_rounded(
                    1.0,
                    scale,
                    &denominator,
                    "0",
                    "1",
                    mode,
                    2000,
                ),
                expected,
                "huge precision must compare the exact rational to its Float neighbor"
            );
        }
    }
}

#[test]
fn quantity_generic_bound_preserves_concrete_unit_and_kind() {
    let src = r#"
#UnitFamily(Length, dimension, base: meter) { meter }
fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q { return value }
fn run() { source :: 3meter; value :: keep(^source); print("{(value.raw())}") }
"#;
    let codes = codes_of(src);
    assert!(codes.is_empty(), "Quantity bounds must accept a determined concrete unit: {codes:?}");
}

#[test]
fn quantity_generic_bound_rejects_wrong_dimension_and_kind() {
    let wrong_dimension = r#"
#UnitFamily(Length, dimension) { meter }
#UnitFamily(Time, dimension) { second }
fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q { return value }
fn run() { source :: 3second; keep(^source) }
"#;
    assert_eq!(codes_of(wrong_dimension), vec!["E0905"]);

    let wrong_kind = r#"
#UnitFamily(Temperature, dimension, base: kelvin) { kelvin celsius(offset: 27315/100) }
fn keep<Q: Quantity<Temperature, .Delta>>(value: ^Q) => Q { return value }
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
pub #UnitFamily(Length, dimension) { meter }
pub fn sample() => Meter { return 2meter }
pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q { return value }
pub fn raw_meter(value: Meter) => Float { return value.raw() }
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
        "pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q { return value }\n",
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(
        &entry,
        r#"
use "units" as units
#UnitFamily(Time, dimension) { second }
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
fn quantity_bounds_accept_open_dimensions_but_reject_unknown_kinds() {
    let open = "fn keep<Q: Quantity<Banana, .Linear>>(value: ^Q) => Q { return value }";
    let (tokens, lex) = jet::Lexer::lex(open);
    assert!(lex.is_empty(), "lex diagnostics: {lex:?}");
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "open dimension names must not come from a compiler table"
    );

    let bad_kind =
        "fn keep<Q: Quantity<Length, .Mystery>>(value: ^Q) => Q { return value }";
    let (tokens, lex) = jet::Lexer::lex(bad_kind);
    assert!(lex.is_empty(), "lex diagnostics: {lex:?}");
    assert!(jet::Parser::parse(&tokens).is_err());
}

#[test]
fn quantity_generic_bounds_are_frozen_into_public_api_identity() {
    let parse = |src: &str| {
        let (tokens, diagnostics) = jet::Lexer::lex(src);
        assert!(diagnostics.is_empty());
        jet::Parser::parse(&tokens).expect("public Quantity generic should parse")
    };
    let length = parse(
        "pub fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q { return value }",
    );
    let time = parse(
        "pub fn keep<Q: Quantity<Time, .Linear>>(value: ^Q) => Q { return value }",
    );
    let length = jet::Publish::ApiFreeze::snapshot_from_items(&length.items, "units", "1.0.0");
    let time = jet::Publish::ApiFreeze::snapshot_from_items(&time.items, "units", "1.0.0");
    assert_eq!(
        length.funcs[0].signature,
        "fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q"
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
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
#Policy(explicit_units)
fn run() {
    total :: 1meter + 1millimeter
    print("{(total.raw())}")
}
"#;
    assert_eq!(codes_of(implicit), vec!["E0127"]);

    let explicit = r#"
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
}
#Policy(explicit_units)
fn run() {
    converted :: Meter.from_millimeter(1000millimeter) ?? panic("exact conversion")
    total :: 1meter + converted
    print("{(total.raw())}")
}
"#;
    assert!(codes_of(explicit).is_empty());

    let module_scoped = r#"
#Policy(explicit_units)
#UnitFamily(Length, dimension, base: meter) { meter millimeter(scale: 1/1000) }
fn run() { total :: 1meter + 1millimeter; print(total.raw()) }
"#;
    assert_eq!(codes_of(module_scoped), vec!["E0127"]);

    let block_scoped = r#"
#UnitFamily(Length, dimension, base: meter) { meter millimeter(scale: 1/1000) }
fn run() {
    #Policy(explicit_units) {
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
#UnitFamily(Length, dimension, base: meter) {
    meter
    thirdish(scale: 2/3)
}
fn run() { value :: 1meter + 1thirdish; print("{(value.raw())}") }
"#;
    assert_eq!(codes_of(rounding), vec!["E0127"]);

    let overflow = format!(
        "#UnitFamily(Length, dimension, base: meter) {{ meter giant(scale: {}) }}\nfn run() {{ value :: 1giant + 1meter; print(\"{{(value.raw())}}\") }}",
        "9".repeat(400)
    );
    assert_eq!(codes_of(&overflow), vec!["E0127"]);

    let explicit_overflow = format!(
        "#UnitFamily(Length, dimension, base: meter) {{ meter giant(scale: {}) }}\nfn run() {{ value :: Meter.from_giant(1giant); print(\"{{(value.raw())}}\") }}",
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
#UnitFamily(Length, dimension, base: meter) {
    meter
    millimeter(scale: 1/1000)
    thirdish(scale: 2/3)
}
fn run() {
    exact :: Meter.from_millimeter(3000millimeter) ?? panic("exact conversion failed")
    inexact :: Meter.from_thirdish(1thirdish) ?? Meter.from_float(-1.0)
    rounded :: Meter.from_thirdish_rounded(1thirdish, .NearestEven, digits: 0) ?? panic("rounded conversion failed")
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
#UnitFamily(Length, dimension, base: meter) {
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
#UnitFamily(Length, dimension) { meter }
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
        "{}\nfn add(a: Usd, b: Usd) => Usd {{ return a + b }}\nfn run() {{ t :: add(Usd.from_float(1.0), Usd.from_float(2.0)); print(\"{{(t.raw())}}\") }}\n",
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
#UnitFamily(Speed) { m_per_s }
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
#UnitFamily(Length, dimension) { meter }
#UnitFamily(Time, dimension) { second }

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
fn quantities_display_units_styles_and_explicit_overrides() {
    let defaults = r#"
#UnitFamily(Length, dimension, base: meter) { meter px(scale: 1) }
#UnitFamily(Time, dimension, base: second) { second }
#UnitFamily(Currency) { usd }

fn run() {
    distance :: 12meter
    elapsed :: 3second
    speed :: distance / elapsed
    pixels :: 766px
    price :: 5usd
    print(distance)
    print("{speed}")
    print(pixels)
    print(price)
    print("{distance#Unit(name)}")
    print("{distance#Unit(bare)}")
    print(distance.raw())
}
"#;
    let (code, stdout) = tir_support::build_and_run("quantity_display_defaults", defaults);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "12 meter\n4 meter/second\n766 px\n5 usd\n12 Meter\n12\n12.0\n"
    );
    if jet_jit::cranelift_host_supported() {
        use jet::JitBackend::JitBackend;
        use jet::JitBackend::RunOutcome;

        let dir = common::unique_tmp("quantity_display_jit");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.jet");
        std::fs::write(&path, defaults).unwrap();
        let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran { stdout, .. } => assert_eq!(
                stdout,
                "12 meter\n4 meter/second\n766 px\n5 usd\n12 Meter\n12\n12.0\n"
            ),
            RunOutcome::Problems(diagnostics) => panic!("JIT rejected unit display: {diagnostics:?}"),
        }
    }

    let explicit = r#"
#UnitFamily(Length, dimension, base: meter) { meter }

impl Meter.Display {
    fn display(self) => String = "custom length"
}

fn run() {
    distance :: 12meter
    print(distance)
    print("{distance}")
    print("{distance#Unit(bare)}")
}
"#;
    let (code, stdout) = tir_support::build_and_run("quantity_display_override", explicit);
    assert_eq!(code, 0);
    assert_eq!(stdout, "custom length\ncustom length\n12\n");

    let dir = common::unique_tmp("quantity_display_override_parity");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, explicit).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("custom unit Display must lower through shared TIR");
    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_program(
        &program,
        &bundle.project_root,
        &mut sink,
        std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        true,
    )
    .expect("custom unit Display must run in the evaluator");
    assert_eq!(sink.stdout, "custom length\ncustom length\n12\n");

    if jet_jit::cranelift_host_supported() {
        use jet::JitBackend::JitBackend;
        use jet::JitBackend::RunOutcome;

        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran { stdout, .. } => {
                assert_eq!(stdout, "custom length\ncustom length\n12\n")
            }
            RunOutcome::Problems(diagnostics) => {
                panic!("JIT rejected custom unit Display: {diagnostics:?}")
            }
        }
    }

    let web_src = r#"
#Target(Web)
#UnitFamily(Length, dimension, base: meter) { meter }

impl Meter.Display {
    fn display(self) => String = "custom length"
}

fn show(distance: Meter) {
    print(distance)
    print("{distance}")
}

fn run() {}
"#;
    let web = jet::compile_web_with_path(&web_src, "quantity_display_override_web.jet")
        .expect("custom unit Display must compile through the web backend")
        .web
        .expect("web compilation must produce artifacts");
    let wasm_dir = common::unique_tmp("quantity_display_override_web_wasm");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_rust = wasm_dir.join("app_wasm.rs");
    let wasm_bin = wasm_dir.join("app.wasm");
    std::fs::write(&wasm_rust, &web.wasm_rust).unwrap();
    let rustc = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
        ])
        .arg(&wasm_rust)
        .arg("-o")
        .arg(&wasm_bin)
        .output()
        .expect("run rustc for custom unit Display web output");
    assert!(
        rustc.status.success(),
        "rustc rejected custom unit Display web output:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    assert!(wasm_bin.is_file(), "web build did not produce a Wasm artifact");
}

#[test]
fn dimensional_quantities_example_stays_in_native_jit() {
    if !jet_jit::cranelift_host_supported() {
        return;
    }
    use jet::JitBackend::JitBackend;
    use jet::JitBackend::RunOutcome;

    let path = "examples/features/types/dimensional_quantities.jet";
    let mut bundle = jet::Loader::load_entry(path).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "dimensional quantities must stay resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle)
        .unwrap_or_else(|reason| panic!("dimensional quantities must JIT-compile: {reason}"));

    jet_jit::reset_jit_trace_for_test();
    let mut backend = jet_jit::CraneliftBackend::new();
    match backend.run(&bundle, false) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(
                stdout,
                "12 meter\n4 meter/second\n12 meter\n766 px\n12 Meter\n12\n12.0\n"
            );
            assert!(stderr.is_empty());
            assert_eq!(exit_code, 0);
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("JIT rejected dimensional quantities: {diagnostics:?}")
        }
    }
    assert!(
        jet_jit::jit_executed_for_test(),
        "dimensional quantities did not execute native JIT code"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test(),
        "dimensional quantities deoptimized to the interpreter"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "dimensional quantities invoked a forbidden fallback"
    );
}

#[test]
fn imported_public_units_keep_display_metadata_across_tiers() {
    let units = r#"
pub #UnitFamily(Length, dimension, base: meter) { meter }

impl Meter.Display {
    fn display(self) => String = "defined in units"
}

pub fn distance() => Meter { return 12meter }
"#;
    let main = r#"
use "measurements" as units

struct Nested {
    values: [[units.Meter]]
}

fn run() {
    distance :: units.distance()
    print(distance)
    print("{distance}")
    print("{distance#Unit(name)}")
    print("{distance#Unit(bare)}")
}
"#;
    let expected = "defined in units\ndefined in units\n12 Meter\n12\n";

    if tir_support::have_rustc() {
        let (code, stdout) = tir_support::build_and_run_multi(
            "imported_unit_display",
            "main.jet",
            &[("measurements.jet", units), ("main.jet", main)],
        );
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }

    let dir = common::unique_tmp("imported_unit_display_tiers");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("measurements.jet"), units).unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, main).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("imported unit display must lower through shared TIR");
    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_program(
        &program,
        &bundle.project_root,
        &mut sink,
        std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        true,
    )
    .expect("imported unit display must run in the evaluator");
    assert_eq!(sink.stdout, expected);

    if jet_jit::cranelift_host_supported() {
        use jet::JitBackend::JitBackend;
        use jet::JitBackend::RunOutcome;

        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, expected),
            RunOutcome::Problems(diagnostics) => {
                panic!("JIT rejected imported unit display: {diagnostics:?}")
            }
        }
    }

    let web_units = r#"
pub #UnitFamily(Length, dimension, base: meter) { meter }

impl Meter.Display {
    fn display(self) => String = "defined in units"
}
"#;
    let web_main = r#"
#Target(Web)
use "web_units" as units

fn show(distance: units.Meter) {
    print(distance)
    print("{distance}")
}

fn run() {}
"#;
    std::fs::write(dir.join("web_units.jet"), web_units).unwrap();
    let web_entry = dir.join("web.jet");
    std::fs::write(&web_entry, web_main).unwrap();
    let web = jet::compile_web(web_entry.to_str().unwrap())
        .expect("imported unit display must compile for web")
        .web
        .expect("web compilation must produce artifacts");
    assert!(
        web.wasm_rust.contains("defined in units"),
        "web output must retain the defining module's Display implementation"
    );
    let wasm_rust = dir.join("imported_unit_app_wasm.rs");
    let wasm_bin = dir.join("imported_unit_app.wasm");
    std::fs::write(&wasm_rust, &web.wasm_rust).unwrap();
    let rustc = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
        ])
        .arg(&wasm_rust)
        .arg("-o")
        .arg(&wasm_bin)
        .output()
        .expect("run rustc for imported unit Display web output");
    assert!(
        rustc.status.success(),
        "rustc rejected imported unit Display web output:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    assert!(wasm_bin.is_file(), "web build did not produce a Wasm artifact");
}

#[test]
fn physical_dimension_mismatch_is_rejected_in_sema() {
    let src = r#"
#UnitFamily(Length, dimension) { meter }
#UnitFamily(Time, dimension) { second }
fn run() { bad :: 1meter + 1second }
"#;
    let codes = codes_of(src);
    assert_eq!(codes, vec!["E0359"], "expected one dimension mismatch, got {codes:?}");
}

#[test]
fn physical_value_cannot_compare_with_scalar() {
    let src = r#"
#UnitFamily(Length, dimension) { meter }
fn run() { bad :: 1meter < 1.0 }
"#;
    assert_eq!(codes_of(src), vec!["E0359"]);
}

#[test]
fn dimension_exponent_limit_is_a_sema_error_not_a_panic() {
    let mut src = String::from("#UnitFamily(Length, dimension) { meter }\nfn run() {\n    q0 :: 1meter\n");
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
#UnitFamily(Length, dimension) { unit }
pub fn sample() => [[String: [Unit]]] { return [["values": [1unit]]] }
pub fn first(groups: [[String: [Unit]]]) => Unit { return ~groups[0]["values"][0] }
"#;
    let time = r#"
#UnitFamily(Time, dimension) { unit }
pub fn sample() => [[String: [Unit]]] { return [["values": [1unit]]] }
pub fn first(groups: [[String: [Unit]]]) => Unit { return ~groups[0]["values"][0] }
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
pub #UnitFamily(Length, dimension, base: meter) { meter millimeter(scale: 1/1000) }
pub fn sample() => Meter { return 1meter }
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
#UnitFamily(Length, dimension) { meter }
#UnitFamily(Time, dimension) { second }
pub fn distance() => Meter { return 12meter }
pub fn elapsed() => Second { return 3second }
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

#[test]
fn local_same_named_family_does_not_inherit_standard_dimension() {
    let src = r#"
#UnitFamily(Length, base: meter) { meter }
fn run() { squared :: 2meter * 3meter; print(squared.raw()) }
"#;
    let mut bundle = {
        let dir = common::unique_tmp("unit_nominal_opt_in");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.jet");
        std::fs::write(&entry, src).unwrap();
        jet::Loader::load_entry(entry.to_str().unwrap()).unwrap()
    };
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let local = bundle.modules[0]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::UnitFamily(family) if family.family == "Length" => Some(family),
            _ => None,
        })
        .unwrap();
    assert!(
        local.resolved_dimension.is_none(),
        "a local same-named family must remain nominal without `dimension`"
    );
}

#[test]
fn standard_units_share_canonical_owner_across_dependency_boundary() {
    let root = common::unique_tmp("standard_unit_dependency");
    let app = root.join("app");
    let dep = root.join("dep");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(
        app.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { dep: ../dep }\n",
    )
    .unwrap();
    std::fs::write(
        app.join("main.jet"),
        "use dep\npub fn local() => Meter { return 1meter }\nfn accept(value: Meter) { print(value.raw()) }\nfn run() { accept(dep.distance()) }\n",
    )
    .unwrap();
    std::fs::write(
        dep.join("pkg.jet"),
        "payload: { name: \"dep\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    std::fs::write(
        dep.join("dep.jet"),
        "pub fn distance() => Meter { return 2meter }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(app.join("main.jet").to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics.is_empty(),
        "ordinary Prelude units have one owner across packages: {diagnostics:?}"
    );
    for module in &bundle.modules {
        let snapshot = jet::Publish::ApiFreeze::snapshot_from_items(
            &module.items,
            &module.alias,
            "0.1.0",
        );
        for function in snapshot.funcs {
            if function.name == "local" || function.name == "distance" {
                assert!(
                    function.signature.contains("package=core.units; family=Length"),
                    "checked standard-unit API identity must use its semantic owner: {}",
                    function.signature
                );
            }
        }
    }
}

#[test]
fn qualified_imported_dimensions_resolve_by_alias_and_unqualified_collisions_fail() {
    let root = common::unique_tmp("qualified_dimensions");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("stock.jet"),
        "pub #UnitFamily(Inventory, dimension, base: item) { item }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("other.jet"),
        "pub #UnitFamily(Inventory, dimension, base: widget) { widget }\n",
    )
    .unwrap();
    let qualified = root.join("qualified.jet");
    std::fs::write(
        &qualified,
        "use \"stock\" as dep\n#UnitFamily(Rate, dimension: dep.Inventory / Time, base: item_per_second) { item_per_second }\nfn run() {}\n",
    )
    .unwrap();
    let qualified_diagnostics = jet::check_with_path(&qualified.to_string_lossy());
    assert!(
        qualified_diagnostics.is_empty(),
        "qualified dimensions must resolve through the written alias: {qualified_diagnostics:?}"
    );

    let ambiguous = root.join("ambiguous.jet");
    std::fs::write(
        &ambiguous,
        "use \"stock\" as stock\nuse \"other\" as other\n#UnitFamily(Rate, dimension: Inventory / Time, base: item_per_second) { item_per_second }\nfn run() {}\n",
    )
    .unwrap();
    let diagnostics = jet::check_with_path(&ambiguous.to_string_lossy());
    assert_eq!(
        diagnostics.iter().map(|diagnostic| diagnostic.code.as_str()).collect::<Vec<_>>(),
        ["E0905"],
        "an unqualified collision must not depend on import order: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].what.contains("ambiguous"),
        "the collision needs a specific ambiguity diagnostic: {diagnostics:?}"
    );
}

#[test]
fn custom_axis_identity_ignores_checkout_root_and_separates_packages() {
    fn resolved_axis(
        root: &std::path::Path,
        package: &str,
    ) -> (jet::AST::Dimension, String) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("pkg.jet"),
            format!("payload: {{ name: \"{package}\", version: \"1.0.0\" }}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("main.jet"),
            "pub #UnitFamily(Inventory, dimension, base: item) { item }\npub fn sample() => Item { return 1item }\nfn run() {}\n",
        )
        .unwrap();
        let mut bundle = jet::Loader::load_entry(root.join("main.jet").to_str().unwrap()).unwrap();
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let dimension = bundle.modules[0]
            .items
            .iter()
            .find_map(|item| match item {
                jet::AST::Item::UnitFamily(family) if family.family == "Inventory" => {
                    family.resolved_dimension.clone()
                }
                _ => None,
            })
            .unwrap();
        let signature = jet::Publish::ApiFreeze::snapshot_from_items(
            &bundle.modules[0].items,
            package,
            "1.0.0",
        )
        .funcs[0]
            .signature
            .clone();
        (dimension, signature)
    }

    let scratch = common::unique_tmp("stable_unit_axis");
    let first = resolved_axis(&scratch.join("checkout-a"), "warehouse");
    let second = resolved_axis(&scratch.join("checkout-b"), "warehouse");
    let distinct = resolved_axis(&scratch.join("checkout-c"), "ledger");
    assert_eq!(first.0, second.0, "checkout roots are not semantic identity");
    assert_eq!(first.1, second.1, "API identity must ignore checkout roots");
    assert_ne!(first.0, distinct.0, "distinct package metadata must not collide");
    assert_ne!(first.1, distinct.1, "distinct package APIs must not collide");
    assert!(
        !first.0.identity().contains(&scratch.to_string_lossy().as_ref()),
        "serialized dimension identity must not expose a checkout path"
    );
}
