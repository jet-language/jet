#[test]
fn authority_is_a_named_prelude_rights_carrier() {
    let source = r#"
struct Holder {
    authority: Authority
}

fn run() {
    authority :: Authority.workspace()
    print("authority")
}
"#;
    let output = jet::compile(source).expect("Authority type should compile");
    assert!(output.rust.contains("pub struct JetAuthority"), "{}", output.rust);
    assert!(
        output
            .rust
            .contains("rights: std::collections::BTreeSet<String>"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
    assert!(
        output.rust.contains("JetAuthority::workspace()"),
        "{}",
        output.rust
    );
}
