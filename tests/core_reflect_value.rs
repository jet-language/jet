//! Runtime-tier coverage for D-ANY-JAI1's canonical `reflect.of` intrinsic.

mod common;
mod tir_support;

#[test]
fn reflect_of_preserves_typed_integer_and_text_values() {
    tir_support::assert_tiers_agree(
        "reflect_of_typed_integer_and_text",
        r#"
use core.reflect as reflect

fn run() {
    integer :: reflect.of(7)
    print(integer.type_name())
    print(integer.display())

    text :: reflect.of("jet")
    print(text.type_name())
    print(text.display())
}
"#,
        "Int\n7\nString\njet\n",
    );
}
