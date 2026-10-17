//! Source-owned encoding hostile cases proved through every hosted execution tier.

mod common;
mod tir_support;

#[test]
fn encoding_foundation_hostile_and_multiline_cases_match_all_tiers() {
    tir_support::assert_tiers_agree(
        "core_encoding_foundations",
        r#"
use core.encoding.base64 as base64
use core.encoding.binary as binary
use core.encoding.hex as hex
use core.encoding.json as json
use core.encoding.xml as xml
use core.encoding.toml as toml
use core.encoding.yaml as yaml

fn run() {
    print(yaml.parse(": value\n") == .Err(_))
    data := [U8]{}
    loop i in 0..<46 -> &data.push(U8{i})
    print(toml.parse("x = bare") == .Err(_))
    print(toml.parse("date = 2024-02-29") != .Err(_))
    print(toml.parse("date = 2023-02-29") == .Err(_))
    print(hex.a2b_uu(hex.b2a_uu(data)) == data)
    print(base64.a85decode("uuuuu") == .Err(_))
    print(base64.b85decode("~~~~~") == .Err(_))
    print(binary.unpack("I", [U8]{0, 0, 0, 0, 0}) == .Err(_))
    print(json.parse("1e9999") == .Err(_))
    print(xml.parse("<1root/>") == .Err(_))
    print(xml.parse("<r>&</r>") == .Err(_))
}
"#,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn source_encoding_error_display_has_one_bundle_implementation() {
    tir_support::assert_tiers_agree(
        "source_encoding_error_display",
        r#"
use core.encoding.[EncodingError, EncodingErrorKind, EncodingFormat]

fn run() {
    err :: EncodingError{
        format: .CBOR, kind: .Syntax, byte_offset: 7,
        line: Val(2), column: Val(3), path: "$[0]",
        reason: "truncated item", cause: None,
    }
    print("{err}")
}
"#,
        "CBOR Syntax at byte 7, line 2, column 3, path $[0]: truncated item\n",
    );
}

#[test]
fn user_duplicate_display_is_rejected_as_e0908() {
    let source = r#"
struct UserMessage {
    text: String
}

impl UserMessage.Display {
    fn display(self) -> String { self.text }
}

impl UserMessage.Display {
    fn display(self) -> String { "duplicate" }
}

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("a user Display impl must remain unique");
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect::<Vec<_>>();
    let codes = errors
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["E0908"], "unexpected diagnostics: {diagnostics:?}");
    assert!(
        errors[0].what.contains("UserMessage")
            && errors[0].what.contains("Display"),
        "duplicate Display diagnostic lost its owner or trait: {diagnostics:?}"
    );
}
