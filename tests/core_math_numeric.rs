//! Numeric CoreLib boundary coverage for the source-owned `core.math` API.

mod common;
mod tir_support;

#[test]
fn core_math_constants_and_rounding_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_numeric_boundaries",
        r#"
use core.math as math

fn run() {
    print(math.pi())
    print(math.round(1.4))
    print(math.round(1.5))
    print(math.round(-1.4))
    print(math.round(-1.5))
    print(math.round(0.5))
    print(math.round(-0.5))
}
"#,
        "3.141592653589793\n1\n2\n-1\n-2\n1\n-1\n",
    );
}

#[test]
fn core_math_exact_int_crosses_native_width_on_every_tier() {
    tir_support::assert_tiers_agree(
        "core_math_exact_int_boundaries",
        r#"
use core.math as math

fn run() {
    print(math.checked_abs(-9223372036854775808) ?? -1)
    print(math.checked_neg(-9223372036854775808) ?? -1)
    print(math.checked_add(9223372036854775807, 1) ?? -1)
    print(math.checked_sub(-9223372036854775808, 1) ?? 0)
    print(math.checked_mul(9223372036854775808, 2) ?? -1)
    print(math.checked_div(-9223372036854775808, -1) ?? -1)
    print(math.checked_rem(-9223372036854775808, -1) ?? -1)
    print(math.checked_pow(2, 100) ?? -1)
    print(math.saturating_add(9223372036854775807, 1))
    print(math.saturating_mul(9223372036854775808, 2))
    print(math.int_pow(2, 100))
    print(math.sum_int([Int]{9223372036854775807, 1}))
    print(math.prod_int([Int]{9223372036854775808, 2}))
    print(math.abs_diff(-9223372036854775808, 9223372036854775808))
    print(math.checked_div(1, 0) == None)
    print(math.checked_pow(2, -1) == None)
    print(math.checked_div(-7, 3) ?? 0)
    print(math.checked_rem(-7, 3) ?? 0)
    floored :: math.div_mod(-7, 3)
    print(floored.quot)
    print(floored.rem)
    euclidean :: math.div_mod(-7, -3)
    print(euclidean.quot)
    print(euclidean.rem)
    truncated :: math.div_rem(-7, 3)
    print(truncated.quot)
    print(truncated.rem)
    wide :: math.div_mod(18446744073709551616, 3)
    print(wide.quot)
}
"#,
        "9223372036854775808\n9223372036854775808\n9223372036854775808\n-9223372036854775809\n18446744073709551616\n9223372036854775808\n0\n1267650600228229401496703205376\n9223372036854775808\n18446744073709551616\n1267650600228229401496703205376\n9223372036854775808\n18446744073709551616\n18446744073709551616\ntrue\ntrue\n-2\n-1\n-3\n2\n3\n2\n-2\n-1\n6148914691236517205\n",
    );
}

#[test]
fn core_math_scalar_predicates_and_comparison_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_scalar_predicates",
        r#"
use core.math as math

fn run() {
    print(math.cmp(math.nan(), 1.0))
    print(math.cmp(1.0, math.nan()))
    print(math.cmp(math.nan(), math.nan()))
    print(math.cmp(-0.0, 0.0))
    print(math.cmp(-2.0, -1.0))
    print(math.cmp(math.infinity(), 1.0))
    print(math.is_finite(math.nan()))
    print(math.is_finite(math.infinity()))
    print(math.is_finite(math.from_bits(1)))
    print(math.is_finite(-0.0))
    print(math.to_bits(math.fabs(-0.0)))
    print(math.fabs(-2.0))
    print(math.cmp(F32{-0.0}, F32{0.0}))
    print(math.cmp(F32{math.nan()}, F32{1.0}))
    print(math.is_finite(F32{math.infinity()}))
    print(math.is_finite(F32{1.0}))
}
"#,
        "1\n-1\n0\n0\n-1\n1\nfalse\nfalse\ntrue\ntrue\n0\n2\n0\n1\nfalse\ntrue\n",
    );
}

#[test]
fn core_math_ambient_transition_and_bit_projection_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_ambient_transition",
        r#"
use core.math as math
use core.math.random as random

fn run() {
    random.seed(1)
    print(random.getrandbits(63))
    random.seed(1)
    print(math.random())
    random.seed(0)
    print(random.getrandbits(1))
    print(random.getrandbits(0))
}
"#,
        "6699714934819013450\n0.7263845487364354\n0\n0\n",
    );
}

#[test]
fn core_math_caller_rng_coin_probability_and_generic_draws_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_math_caller_rng_known_answers",
        r#"
use core.math as math
use core.math.random as random

fn draw(stream: &random.RNG) {
    print(&stream.bool())
    print(&stream.bool(0.25))
    bytes :: &stream.bytes(6)
    loop byte in bytes -> print(byte)
    print(&stream.pick([String]{"a", "b", "c", "d"}) ?? "empty")
    print(&stream.weighted_pick([String]{"a", "b", "c", "d"}, [Float]{math.nan(), -1.0, 0.25, 1.75}) ?? "empty")
    sample :: &stream.sample([String]{"a", "b", "c", "d", "e"}, 3)
    loop value in sample -> print(value)
    items := [String]{"a", "b", "c", "d", "e"}
    &stream.shuffle(&items)
    loop value in items -> print(value)
    child := &stream.split()
    print(&child.int(1, 100))
    print(&stream.bool(0.0))
    print(&stream.bool(1.0))
    print(&stream.bool(math.nan()))
    print(&stream.int(1, 100))
}
fn run() {
    stream := random.rng(42)
    draw(&stream)
}
"#,
        "true\ntrue\n82\n148\n242\n6\n93\n164\nb\nd\nb\nc\nd\nb\nd\na\ne\nc\n91\nfalse\ntrue\nfalse\n10\n",
    );
}

#[test]
fn core_math_caller_rng_shuffle_moves_close_owners_without_clone_or_early_close() {
    tir_support::assert_tiers_agree(
        "core_math_caller_rng_close_owner_shuffle",
        r#"
use core.math.random as random

struct Ticket { id: Int, payload: [U8] }
impl Ticket.Close {
    fn close(^self) { print("close token {self.payload.len()}") }
}
fn shuffled() {
    stream := random.rng(42)
    owners := [Ticket]{
        Ticket{id: 1, payload: [U8]{11}},
        Ticket{id: 2, payload: [U8]{22}},
        Ticket{id: 3, payload: [U8]{33}},
    }
    &stream.shuffle(&owners)
    loop owner in owners -> print("{owner.id}:{owner.payload[0]}")
}
fn run() {
    shuffled()
    print("released")
}
"#,
        "2:22\n1:11\n3:33\nclose token 1\nclose token 1\nclose token 1\nreleased\n",
    );
}

#[test]
fn core_math_rng_public_spellings_and_explicit_copies_share_one_nominal() {
    tir_support::assert_tiers_agree(
        "core_math_rng_public_nominal_identity",
        r#"
use core.math.random as random
use core.testing as testing

struct Envelope { stream: testing.RNG }
fn identity<T>(value: ^T) -> T { return value }
fn into_testing(value: ^RNG) -> testing.RNG { return value }
fn into_random(value: ^testing.RNG) -> random.RNG { return value }
fn into_bare(value: ^random.RNG) -> RNG { return value }
fn draw(stream: &RNG) {
    print(&stream.bool())
    print(&stream.bool(0.25))
}
fn run() {
    original := random.rng(42)
    independent := ~original
    envelope := Envelope{stream: into_testing(identity<RNG>(^original))}
    round_trip := into_bare(into_random(^envelope.stream))
    draw(&round_trip)
    draw(&independent)
    print(round_trip)
    print(independent)
}
"#,
        "true\ntrue\ntrue\ntrue\nRNG { .. }\nRNG { .. }\n",
    );
}

#[test]
fn core_math_rng_loader_trigger_has_a_bare_positive_and_unrelated_negative() {
    for (label, source, expected) in [
        ("bare", "fn accepts(stream: &RNG) {}\nfn run() {}\n", true),
        ("unrelated", "fn run() { print(\"ordinary\") }\n", false),
    ] {
        let scratch = common::Scratch::new(&format!("core_rng_loader_{label}"));
        tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
        let entry = scratch.path.join("run.jet");
        std::fs::write(&entry, source).unwrap();
        let bundle = jet::Loader::load_entry(entry.to_str().unwrap()).expect("loader fixture parses");
        assert_eq!(
            bundle.modules.iter().any(|module| module.path.ends_with("Core/Math/Random.jet")),
            expected,
            "{label}: an unrelated program must not preload RNG",
        );
    }
}

#[test]
fn core_math_rng_spelling_diagnostics_are_identical() {
    let mut expected = None;
    for spelling in ["RNG", "testing.RNG", "random.RNG"] {
        let source = format!(
            "use core.math.random as random\nuse core.testing as testing\n\
             fn accepts(stream: {spelling}) {{}}\nfn run() {{ accepts(\"wrong\") }}\n"
        );
        let diagnostics = tir_support::compile_source("rng_type_diagnostic", &source)
            .expect_err("a String is not an RNG");
        let errors: Vec<_> = diagnostics.into_iter()
            .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
            .map(|diagnostic| (diagnostic.code, diagnostic.what, diagnostic.why, diagnostic.fix))
            .collect();
        assert!(errors.iter().any(|(_, what, _, _)| what.contains("RNG")), "{errors:#?}");
        if let Some(expected) = &expected {
            assert_eq!(&errors, expected, "{spelling}");
        } else {
            expected = Some(errors);
        }
    }
}

#[test]
fn core_math_rng_is_not_implicitly_copyable() {
    let source = r#"
use core.math.random as random
fn consume(stream: ^RNG) {}
fn run() {
    stream := random.rng(42)
    consume(^stream)
    print(&stream.bool())
}
"#;
    let diagnostics = tir_support::compile_source("rng_use_after_move", source)
        .expect_err("caller-owned RNG must retain the old non-Copy classification");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0121"), "{diagnostics:#?}");
}

#[test]
fn core_math_rng_source_does_not_shadow_a_local_nominal() {
    tir_support::assert_tiers_agree(
        "core_math_rng_local_nominal_precedence",
        r#"
use core.math.random as random
struct RNG { value: Int }
fn accepts(value: RNG) -> Int { return value.value }
fn run() {
    stream := random.rng(42)
    print(&stream.bool(0.25))
    print(accepts(RNG{value: 7}))
}
"#,
        "false\n7\n",
    );
}

#[test]
fn core_math_rng_single_imports_preserve_bare_nominal_identity() {
    for (label, imports, annotation, constructor) in [
        ("random_only", "use core.math.random as random", "random.RNG", "random.rng(42)"),
        ("testing_only", "use core.testing as testing", "testing.RNG", "testing.fake_rng(42)"),
        ("both", "use core.math.random as random\nuse core.testing as testing", "testing.RNG", "random.rng(42)"),
    ] {
        let source = format!(
            "{imports}\nfn bare(value: ^{annotation}) -> RNG {{ return value }}\n\
             fn run() {{ stream := bare({constructor})\nprint(&stream.bool())\nprint(&stream.bool(0.25)) }}\n"
        );
        tir_support::assert_tiers_agree(
            &format!("core_math_rng_import_{label}"),
            &source,
            "true\ntrue\n",
        );
    }
}

#[test]
fn core_math_rng_alias_resolution_does_not_recanonicalize_date() {
    tir_support::assert_tiers_agree(
        "core_math_rng_date_annotation_regression",
        r#"
use core.math.random as random
use core.testing as testing
use core.time as time
struct Dated { day: Date, stream: testing.RNG }
fn day(value: Date) -> Date { return value }
fn run() {
    dated := Dated{day: day(time.new(2024, 2, 29)), stream: random.rng(42)}
    print(dated.day)
    print(dated.day.year())
    print(&dated.stream.bool())
}
"#,
        "2024-02-29\n2024\ntrue\n",
    );
}

#[test]
fn core_math_rng_unlisted_core_alias_does_not_become_a_bare_prelude_type() {
    let scratch = common::Scratch::new("core_rng_unlisted_alias");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("run.jet");
    std::fs::write(
        &entry,
        "use core.testing as testing\nfn accepts(value: UnlistedStream) {}\nfn run() {}\n",
    ).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect("load the real embedded testing module before injecting the negative alias");
    let testing = bundle.modules.iter_mut()
        .find(|module| module.path.ends_with("Core/Testing/Testing.jet"))
        .expect("core.testing is loaded");
    let mut alias = testing.items.iter().find_map(|item| match item {
        jet::AST::Item::TypeAlias(alias) if alias.name == "RNG" => Some(alias.clone()),
        _ => None,
    }).expect("the admitted RNG re-export is a source alias");
    alias.name = "UnlistedStream".to_string();
    testing.items.push(jet::AST::Item::TypeAlias(alias));
    let diagnostics = jet::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(
        diagnostics.iter().any(|diagnostic|
            diagnostic.severity == jet::Diagnostics::Severity::Error
                && diagnostic.what.contains("UnlistedStream")),
        "an unlisted public Core alias must remain qualified: {diagnostics:#?}",
    );
    assert!(
        !diagnostics.iter().any(|diagnostic|
            diagnostic.severity == jet::Diagnostics::Severity::Error
                && diagnostic.what.contains("RNG")),
        "the negative alias must not break the admitted RNG alias: {diagnostics:#?}",
    );
}
