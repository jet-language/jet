//! Card #2253: the one-million-read Reader witness is an executable I9
//! contract. Its output must agree on release/AOT, default JIT, and the forced
//! interpreter; the generated-code shape contract lives beside the TIR tests.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn reader_hot_loop_one_million_reads_matches_all_tiers() {
    let expected = include_str!("../examples/features/expected/parsing/reader_hot_loop.out");
    tir_support::assert_example_cli_tiers_agree("parsing/reader_hot_loop", expected);
}

#[test]
fn reader_region_short_input_keeps_the_fallible_fallback() {
    let source = r#"
fn run() {
    reader :: Reader.over([U8]{7})
    loop _ in 0..<2 {
        value :: reader.read_u16_le() ?? 99
        print(value)
    }
}
"#;
    tir_support::assert_tiers_agree("reader_region_short", source, "99\n99\n");
}
