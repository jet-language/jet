//! Source-owned Argon2id known-answer coverage across execution tiers.

mod common;
mod tir_support;

#[test]
fn argon2id_parallel_lanes_match_rfc_vector_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_crypto_argon2id_parallel",
        r#"
use core.crypto as crypto
use core.crypto.expert as expert

fn run() {
    password :: crypto.Secret.from_text("password")
    #Unsafe("fixed Argon2id parallel-lane known-answer vector") {
        derived :: expert.argon2id(password, "somesalt".bytes(), 8192, 2, 2, 32) ?? panic("argon2id")
        print(expert.secret_bytes(derived) == [U8]{0xd1, 0xe8, 0x3a, 0xa7, 0x38, 0x3f, 0x70, 0x87, 0x3a, 0x17, 0x1b, 0x45, 0x32, 0x06, 0xa0, 0x0d, 0xac, 0xa5, 0x34, 0x0f, 0x35, 0x05, 0x54, 0x2e, 0x17, 0xb4, 0x1e, 0xd6, 0x2d, 0x3b, 0x11, 0x0e})
    }
}
"#,
        "true\n",
    );
}
