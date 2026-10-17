mod common;

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
fn secret_decode_uses_the_source_owned_text_codec() {
    let output = jet::compile(
        r#"
use core.crypto as crypto
use core.encoding.json as json

#[Decode]
struct Config {
    #Redact password: crypto.Secret
}

fn run() {
    value :: json.decode<Config>("{{\"password\":\"credential\"}}") ?? return
    if value.password.bytes != "credential".bytes() -> panic("secret decode bytes")
    if crypto.Secret.decode(DataTree.Int(42)) == .Ok(_) -> panic("non-text secret accepted")
}
"#,
    )
    .expect("Secret Decode must lower through its checked source implementation");
    assert!(
        output.rust.contains("impl __jet_Decode for __jet__x3ccorelib_x3e_sCore_scrypto_c_cCore_scrypto_scrypto_djet_c_cSecret"),
        "the source Secret must own the generated Decode implementation"
    );
}

#[test]
fn a_user_secret_name_does_not_acquire_a_builtin_decoder() {
    let errors = jet::compile(
        r#"
use core.encoding.json as json
#!Codable
struct Secret { bytes: [U8] }
fn run() { value :: json.decode<Secret>("\"credential\"") ?? return }
"#,
    )
    .expect_err("a user Secret needs its own Decode implementation");
    assert!(errors.iter().any(|error| error.code == "E2411"));
}

#[test]
fn source_secret_decode_does_not_enable_secret_encoding() {
    let errors = jet::compile(
        r#"
use core.crypto as crypto
use core.encoding.json as json
fn run() {
    secret :: crypto.Secret.from_text("credential")
    print(json.to_string(secret))
}
"#,
    )
    .expect_err("restoring the configuration decoder must not serialize secret bytes");
    assert!(errors.iter().any(|error| error.code == "E2411"));
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

/// Observe the still-live allocation at the volatile wipe, before Close frees it.
/// Reading a raw pointer after Close would instead be a use-after-free.
#[test]
fn secret_close_zeroizes_the_original_allocation() {
    let output = jet::compile(
        r#"
use core.crypto as crypto

fn scoped(seed: ^[U8]) {
    secret := crypto.Secret{bytes: ^seed}
    close(^secret)
}

fn run() { scoped(^[U8]{1, 2, 3}) }
"#,
    )
    .expect("a consuming Secret.Close should compile");
    let header_end = output.rust.find("_cscoped(__jet_seed").expect("scoped function emitted");
    let header_start = output.rust[..header_end].rfind("fn ").expect("scoped header");
    let scoped = &output.rust[header_start + 3..header_end + "_cscoped".len()];
    let mut rust = output.rust.replacen("fn main() {", "fn __original_main() {", 1);
    rust.push_str(&format!(
        r#"
fn main() {{
    let bytes = vec![0x53u8, 0x45, 0x43, 0x52, 0x45, 0x54];
    let original = bytes.as_ptr() as usize;
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = observed.clone();
    jet_crypto_entropy_set_zeroize_test_observer(move |wiped| {{
        assert_eq!(wiped.as_ptr() as usize, original, "Close wiped a copy");
        assert_eq!(wiped, &[0; 6], "original allocation retained secret bytes");
        seen.set(true);
    }});
    {scoped}(bytes);
    jet_crypto_entropy_clear_zeroize_test_observer();
    assert!(observed.get(), "Close never reached the volatile wipe");
}}
"#
    ));
    let root = common::unique_tmp("jet_secret_close_original_allocation");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("main.rs");
    let binary = root.join("main");
    std::fs::write(&source, rust).unwrap();
    let built = std::process::Command::new("rustc")
        .args(["--edition", "2021", "--cfg", "test"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(built.status.success(), "generated Close rejected:\n{}", String::from_utf8_lossy(&built.stderr));
    let ran = std::process::Command::new(&binary).output().unwrap();
    assert!(ran.status.success(), "original allocation was not zeroized:\n{}", String::from_utf8_lossy(&ran.stderr));
}
