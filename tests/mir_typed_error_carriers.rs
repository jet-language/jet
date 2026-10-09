mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const BASE64_TYPED_ERROR_SOURCE: &str = r#"
use core.encoding.base64 as api

fn report_standard(text: String, expected: U8) {
    if api.decode(text) == {
        .Ok(bytes) -> {
            print(bytes.len())
            print(bytes[0])
            print(bytes[0] == expected)
        }
        .Err(error) -> print(error.reason)
    }
}

fn report_url(text: String, expected: U8) {
    if api.decode_url(text) == {
        .Ok(bytes) -> {
            print(bytes.len())
            print(bytes[0])
            print(bytes[0] == expected)
        }
        .Err(error) -> print(error.reason)
    }
}

fn run() {
    print(api.is_base64("jet"))
    print(api.is_base64("Zg=="))
    report_standard("Zg==", U8{102})
    report_standard("Zg=", U8{102})
    report_standard("a?==", U8{102})
    report_standard("Zh==", U8{102})
    report_standard("-w", U8{251})
    report_url("-w", U8{251})
    report_url("-w==", U8{251})
}
"#;

const OPTIONAL_FOLD_ELIGIBILITY_SOURCE: &str = r#"
use core.encoding.base64 as api
TEXT :: prep { "Zg==" }

fn report_shadowed_local(text: String) {
    input :: text.copy()
    if api.decode(input) == {
        .Ok(bytes) -> print(bytes.len())
        .Err(error) -> print(error.reason)
    }
}

fn report_reader_progress() {
    reader :: Reader.over([U8]{7, 9})
    first :: reader.read_u8() ?? U8{0}
    second :: reader.read_u8() ?? U8{0}
    print(first)
    print(second)
}

fn run() {
    print(TEXT)
    report_shadowed_local("a?==")
    report_reader_progress()
}
"#;

#[test]
fn optional_folds_respect_runtime_shadowing_and_reader_invalidation() {
    tir_support::assert_tiers_agree(
        "optional_fold_eligibility",
        OPTIONAL_FOLD_ELIGIBILITY_SOURCE,
        "Zg==\ninvalid base64 at byte 1: byte 0x3F is not in the standard base64 alphabet\n7\n9\n",
    );
}

#[test]
fn base64_typed_error_carrier_is_consistent_across_i9_tiers() {
    tir_support::assert_tiers_agree(
        "base64_typed_error_carrier",
        BASE64_TYPED_ERROR_SOURCE,
        "false\ntrue\n1\n102\ntrue\ninvalid base64 at byte 2: expected 1 padding characters\ninvalid base64 at byte 1: byte 0x3F is not in the standard base64 alphabet\ninvalid base64 at byte 1: non-zero unused bits\ninvalid base64 at byte 0: byte 0x2D is not in the standard base64 alphabet\n1\n251\ntrue\ninvalid base64url at byte 2: padding is not allowed\n",
    );
}

#[test]
fn generic_collection_kernels_adapt_jet_callable_carriers() {
    tir_support::assert_tiers_agree(
        "generic_collection_host_callbacks",
        r#"
fn run() {
    left :: [String:Int]{"x": 10, "y": 1}
    right :: [String:Int]{"x": 4, "z": 7}
    conflict :: (key: String, a: Int, b: Int) -> a + b
    merged :: left.merge(right, conflict: conflict)
    print(merged.get("x") ?? 0)
    print(merged.get("y") ?? 0)
    print(merged.get("z") ?? 0)
    values :: [1, 2, 3, 4]
    parts :: values.split(2)
    print(parts.left)
    print(parts.right)
}
"#,
        "14\n1\n7\n[1, 2]\n[3, 4]\n",
    );
}

#[test]
fn exact_radix_receiver_uses_the_owned_int_argument_abi() {
    tir_support::assert_tiers_agree(
        "exact_radix_receiver_argument",
        r#"
fn parse(text: String, radix: Int) -> Int String! {
    Int.from_radix(text, radix)
}
fn run() {
    print(parse("ff", 16) ?? 0)
    print(parse("ff", 2) ?? 0)
}
"#,
        "255\n0\n",
    );
}
