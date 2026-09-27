//! Source-owned crypto entropy and constant-time boundary proofs across tiers.

mod common;
mod tir_support;

#[test]
fn crypto_random_boundaries_preserve_error_carriers_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_crypto_random_boundaries",
        r#"
use core.crypto as crypto
use core.crypto.random as random

fn run() {
    if random.token_hex(-1) == {
        .Err(.Length) -> print("hex:length")
        else -> print("hex:wrong-error")
    }
    if random.token_urlsafe(-1) == {
        .Err(.Length) -> print("url:length")
        else -> print("url:wrong-error")
    }
    if random.token_hex(0) == {
        .Ok(value) -> print(value == "")
        else -> print(false)
    }
    if random.token_urlsafe(0) == {
        .Ok(value) -> print(value == "")
        else -> print(false)
    }
    if random.int_range(4, 4) == {
        .Err(.Length) -> print("range:length")
        else -> print("range:wrong-error")
    }
    if random.int_range(0, 18446744073709551617) == {
        .Err(.Length) -> print("range:wide")
        else -> print("range:wrong-error")
    }

    print(crypto.constant_time_equal_bytes([U8]{1, 2, 3}, [U8]{1, 2, 3}))
    print(crypto.constant_time_equal_bytes([U8]{1, 2, 3}, [U8]{1, 2, 4}))
    print(crypto.constant_time_equal_bytes([U8]{1, 2, 3}, [U8]{1, 2}))
    print(random.compare_digest([U8]{}, [U8]{}))
    print(random.compare_digest([U8]{1}, [U8]{}))
}
"#,
        "hex:length\nurl:length\ntrue\ntrue\nrange:length\nrange:wide\ntrue\nfalse\nfalse\ntrue\nfalse\n",
    );
}

#[test]
fn crypto_random_valid_ranges_stay_inside_the_requested_interval_on_all_tiers() {
    tir_support::assert_tiers_agree(
        "core_crypto_random_range",
        r#"
use core.crypto.random as random

fn run() {
    value :: random.int_range(-17, 17) ?? return
    print(value >= -17 && value < 17)
    bits :: random.randbits(13) ?? return
    print(bits >= 0 && bits < 8192)
}
"#,
        "true\ntrue\n",
    );
}

#[test]
fn crypto_aead_tampering_is_rejected_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_crypto_aead_tamper",
        r#"
use core.crypto.expert as expert

fn run() {
    key := [U8]{
        0, 1, 2, 3, 4, 5, 6, 7,
        8, 9, 10, 11, 12, 13, 14, 15,
        16, 17, 18, 19, 20, 21, 22, 23,
        24, 25, 26, 27, 28, 29, 30, 31,
    }
    nonce := [U8]{
        0, 1, 2, 3, 4, 5, 6, 7,
        8, 9, 10, 11, 12, 13, 14, 15,
        16, 17, 18, 19, 20, 21, 22, 23,
    }
    #Unsafe("fixed AEAD tamper vector") {
        sealed :: expert.xchacha20poly1305_seal(key, nonce, [U8]{1, 2, 3}, [U8]{4, 5}) ?? return
        tampered := sealed
        tampered[0] = U8{Int.from_u8(tampered[0]) ~| 1}
        if expert.xchacha20poly1305_open(key, nonce, tampered, [U8]{4, 5}) == {
            .Err(_) -> print("tamper:rejected")
            else -> print("tamper:wrong-error")
        }
    }
}
"#,
        "tamper:rejected\n",
    );
}

#[test]
fn immutable_integer_array_constants_preserve_values_across_tiers() {
    tir_support::assert_tiers_agree(
        "module_integer_array_constants",
        r#"

C_WORDS :: [Int]{17, 18446744073709551615}
C_BYTES :: [U8]{0, 127, 255}

fn run() {
    print(C_WORDS[0])
    print(C_WORDS[1])
    print(Int.from_u8(C_BYTES[1]))
    print(Int.from_u8(C_BYTES[2]))
}
"#,
        "17\n18446744073709551615\n127\n255\n",
    );
}

#[test]
fn module_array_constants_reject_computed_elements_in_sema() {
    let diagnostics = tir_support::compile_source(
        "computed_module_array_constant",
        r#"
BAD_TABLE :: [Int]{1 + 2}
fn run() {}
"#,
    )
    .expect_err("computed module array elements must be rejected by sema");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0109")
        .expect("computed module array elements must produce E0109");
    assert_eq!(
        diagnostic.what,
        "A module binding must use a supported scalar, string, or immutable integer-array literal"
    );
    assert_eq!(
        diagnostic.why,
        "Module globals need a value shape that every execution tier can initialize before `fn run`"
    );
    assert_eq!(
        diagnostic.fix,
        "Use an integer, float, bool, char, or immutable string literal, or an immutable `[Int]`/`[U8]` literal with integer-literal elements"
    );
}
