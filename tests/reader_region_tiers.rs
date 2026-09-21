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
#[test]
fn bounded_fixed_width_to_int_keeps_the_native_carrier() {
    let source = r#"
fn run() {
    values :: [U8]{ 7 }
    loop value in values {
        widened :: Int.from_u8(value)
        print(widened)
    }
}
"#;
    tir_support::assert_tiers_agree("bounded_int_cast", source, "7\n");
}

#[test]
fn reader_u32_to_int_modulo_uses_bounded_native_rail() {
    let source = r#"
fn run() {
    reader :: Reader.over([U8]{9, 0, 0, 0})
    value :: Int.from_u32(reader.read_u32_le() ?? 0)
    print(value % 7)
}
"#;
    tir_support::assert_tiers_agree("reader_u32_int_modulo", source, "2\n");
}

#[test]
fn reader_over_moves_an_owned_bytes_last_use() {
    let src = r#"
fn run() {
    bytes :: [U8]{7, 9}
    reader :: Reader.over(bytes)
    print(reader.remaining())
}
"#;
    tir_support::assert_tiers_agree("reader_over_owned_last_use", src, "2\n");
}

#[test]
fn reader_over_reuses_the_borrowing_clone_when_bytes_are_live() {
    let src = r#"
fn run() {
    bytes :: [U8]{7, 9}
    reader :: Reader.over(bytes)
    print(bytes.len())
    print(reader.remaining())
}
"#;
    tir_support::assert_tiers_agree("reader_over_reused_bytes", src, "2\n2\n");
}

#[test]
fn reader_over_rejects_a_live_view_alias() {
    let src = r#"
fn run() {
    bytes := [U8]{7, 9}
    alias :: bytes[0..1]
    reader :: Reader.over(bytes)
    print(alias.len())
    print(reader.remaining())
}
"#;
    tir_support::assert_tiers_agree("reader_over_live_alias", src, "2\n2\n");
}

#[test]
fn reader_over_rejects_a_later_capture() {
    let src = r#"
fn run() {
    bytes :: [U8]{7, 9}
    reader :: Reader.over(bytes)
    _keep :: () -> bytes.len()
    print(reader.remaining())
}
"#;
    tir_support::assert_tiers_agree("reader_over_later_capture", src, "2\n");
}

#[test]
fn reader_fixed_frame_and_byte_checksum_keep_one_native_shape() {
    let source = r#"
fn run() {
    payload :: [U8]{7, 1, 0x34, 0x12, 1, 0, 0, 0}
    checksum := U64{0}
    loop byte in payload -> checksum += U64.from_u8(byte)
    reader :: Reader.over(payload)
    frame :: reader.take_pattern([U8]{"{channel:U8}{flags:U8}{sample:U16le}{tick:U32le}"}) ?? panic("short frame")
    print(checksum)
    print(frame.channel)
    print(frame.flags)
    print(frame.sample)
    print(frame.tick)
    print(reader.remaining())
}
"#;
    tir_support::assert_tiers_agree(
        "reader_fixed_frame_checksum",
        source,
        "79\n7\n1\n4660\n1\n0\n",
    );
}
