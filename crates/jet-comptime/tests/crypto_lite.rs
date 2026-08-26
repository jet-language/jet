#[path = "../src/Comptime/CryptoLite/Aes256Gcm.rs"]
mod aes256gcm;

#[path = "../src/Comptime/CryptoLite/Argon2id.rs"]
mod argon2id;

const AES_SOURCE: &str = include_str!("../src/Comptime/CryptoLite/Aes256Gcm.rs");
const ARGON2_SOURCE: &str = include_str!("../src/Comptime/CryptoLite/Argon2id.rs");

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("invalid hex test vector"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}

#[test]
fn aes256gcm_known_answer_has_no_secret_table_or_ghash_branch() {
    let key = hex("b52c505a37d78eda5dd34f20c22540ea1b58963cf8e5bf8ffa85f9f2492505b4");
    let nonce = hex("516c33929df5a3284ff463d7");
    let expected = hex("bdc1ac884d332457a1d2664f168c76f0");
    let sealed = aes256gcm::seal(&key, &nonce, &[], &[]).unwrap();
    assert_eq!(sealed, expected);
    assert_eq!(aes256gcm::open(&key, &nonce, &sealed, &[]).unwrap(), Vec::<u8>::new());

    assert!(!AES_SOURCE.contains("SBOX["), "secret-indexed AES table was reintroduced");
    let ghash = AES_SOURCE
        .split_once("fn ghash_mul")
        .and_then(|(_, rest)| rest.split_once("fn ghash("))
        .map(|(body, _)| body)
        .expect("GHASH implementation must remain present");
    assert!(!ghash.contains("if "), "GHASH must not branch on secret state");
}

#[test]
fn argon2id_matches_the_canonical_expert_known_answer() {
    let actual = argon2id::hash(b"password", b"somesalt", 65_536, 2, 1, 32).unwrap();
    let expected = hex("09316115d5cf24ed5a15a31a3ba326e5cf32edc24702987c02b6566f61913cf7");
    assert_eq!(actual, expected);
    assert!(
        !ARGON2_SOURCE.contains("Simplified independent address generation"),
        "the non-standard address generator must not return"
    );
    assert!(
        ARGON2_SOURCE.contains("update_address_block")
            && ARGON2_SOURCE.contains("address_block[address_index]")
            && ARGON2_SOURCE.contains("area as u64 * x"),
        "the standard address generator must be wired into the block loop"
    );
}
