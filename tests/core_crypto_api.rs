#[test]
fn imported_secret_type_keeps_its_static_conversion_apis() {
    let output = jet::compile(
        r#"
use core.crypto as crypto
use core.crypto.[Secret]

fn run() {
    text_secret :: Secret.from_text("mail credential")
    byte_secret :: Secret.from_bytes([0x41, 0x42])
    qualified_text_secret :: crypto.Secret.from_text("module-qualified")
    qualified_byte_secret :: crypto.Secret.from_bytes([0x43, 0x44])
}
"#,
    )
    .expect("selectively imported Secret constructors should compile");

    assert!(
        output
            .lints
            .iter()
            .all(|diagnostic| diagnostic.code != "L0103"),
        "used Secret static conversions must mark the selective import as used: {:?}",
        output.lints
    );
    assert!(
        output.rust.contains("jet_crypto_secret_from_text_impl"),
        "text conversion must lower through the canonical crypto route"
    );
    assert!(
        output.rust.contains("jet_crypto_secret_from_bytes_impl"),
        "byte conversion must lower through the canonical crypto route"
    );
}

#[test]
fn crypto_file_helpers_accept_typed_paths_with_mixed_call_labels() {
    let _output = jet::compile(
        r#"
use core.crypto as crypto

fn run() {
    source :: Path.from("plain.txt")
    sealed :: Path.from("sealed.bin")
    identity :: crypto.X25519SecretKey.new_random() ?? return
    crypto.file_seal([identity.public_key()], source: source, destination: sealed) ?? return
    crypto.file_open(&identity, source: sealed, destination: source) ?? return
}
"#,
    )
    .expect("crypto file helpers should accept Path and mixed positional/named arguments");
}
