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

/// The body of the first emitted item whose header contains `header`.
fn emitted_body<'a>(rust: &'a str, header: &str) -> &'a str {
    let start = rust
        .find(header)
        .unwrap_or_else(|| panic!("emitted Rust has no `{header}`"));
    let rest = &rust[start..];
    &rest[..rest.find("\n}\n").expect("emitted item closes")]
}

/// D-SHAPE-RESOURCE1=A: a `Secret` still live at scope end is closed, and its
/// `Close` hands the owned bytes to the one vetted volatile wipe.
#[test]
fn live_secret_scope_exit_closes_through_the_zeroize_kernel() {
    let output = jet::compile(
        r#"
use core.crypto as crypto

fn scoped(seed: [U8]) {
    secret :: crypto.Secret{bytes: seed}
    print("secret made")
}

fn run() { scoped([U8]{1, 2, 3}) }
"#,
    )
    .expect("a scoped Secret should compile");
    let rust = &output.rust;
    let secret = "__jet__x3ccorelib_x3e_sCore_scrypto_c_cCore_scrypto_scrypto_djet_c_cSecret";

    let close = emitted_body(rust, &format!("impl __jet_Close for {secret} {{"));
    assert!(
        close.contains("jet_crypto_zeroize("),
        "Secret.close must wipe through jet_crypto_zeroize:\n{close}"
    );

    let scoped = emitted_body(rust, "_cscoped(__jet_seed");
    assert!(
        scoped.contains(&format!("<{secret} as __jet_Close>::close(")),
        "the scope exit must close the live Secret, not drop it:\n{scoped}"
    );
}
