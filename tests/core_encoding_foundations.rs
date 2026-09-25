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
    loop i in 0..<46 -> data.push(U8{i})
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
