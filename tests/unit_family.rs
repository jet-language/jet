//! D-QUAL3 (ratified 2026-06-24): unit families, `#UnitFamily(name) { m, … }`.
//!
//! Each member mints one `#Numeric` distinct type erasing to `Float`
//! (`usd` → `Usd`), so signatures read in plain English and the compiler keeps
//! the units from mixing. This is pure sugar over the D-DIST1/D-DIST3 distinct
//! machinery: construct with `Usd(value)`, same-unit arithmetic stays in the
//! unit, `.raw()` strips it, and cross-unit mixing is E0127 (the distinct
//! same-type arithmetic rule). The family erases in codegen (I3).

const FAMILY: &str = r#"
#UnitFamily(currency) { usd, eur }
"#;

fn codes_of(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

/// A member name is usable as a distinct type in a signature; construction,
/// same-unit arithmetic and `.raw()` all compile cleanly.
#[test]
fn same_unit_arithmetic_compiles() {
    let src = format!(
        "{}\nfn add(a: Usd, b: Usd) -> Usd {{ return a + b }}\nfn main() {{ t #= add(Usd(1.0), Usd(2.0)); print(\"{{(t.raw())}}\") }}\n",
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
        "{}\nfn main() {{ bad #= Usd(1.0) + Eur(2.0); print(\"{{(bad.raw())}}\") }}\n",
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
        "{}\nfn take(p: Usd) {{ print(\"{{(p.raw())}}\") }}\nfn main() {{ take(9.99) }}\n",
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
#UnitFamily(speed) { m_per_s }
fn main() {
    v #= MPerS(3.0)
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
        "{}\nfn main() {{ t #= Usd(1.0); print(\"{{(t.raw())}}\") }}\n",
        FAMILY
    );
    let out = jet::compile(&src).expect("should compile");
    assert!(
        !out.rust.contains("unsafe"),
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
