//! #2517 S5a: the self-hosted record codec and package keys write the same
//! bytes as the Rust ones.
//!
//! `Compiler/JetFoundation/Source/Record/RecordCodec.jet` and
//! `PackageIdentity.jet` mirror `jet_foundation::RecordCodec` and
//! `jet_foundation::PackageIdentity`. This test renders one set of inputs as
//! Jet literals, runs the Jet implementation with the freshly built `jet`, and
//! compares every output line with the Rust result for the same input:
//! encoded record bytes, decode verdicts (including the exact rejection text
//! for malformed records), and package keys.

mod common;
use common::Scratch;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::process::Command;

use jet_foundation::PackageIdentity::{package_check_key, source_digest, target_facts_digest};
use jet_foundation::RecordCodec::{decode_record, encode_record, RecordSection, RecordValue};
use jet_foundation::SHA256::sha256;

/// The Jet sources under test, in dependency order.
const JET_SOURCES: &[&str] = &[
    "Compiler/JetFoundation/Source/Hash/SHA256.jet",
    "Compiler/JetFoundation/Source/Record/RecordCodec.jet",
    "Compiler/JetFoundation/Source/Record/PackageIdentity.jet",
];

/// Helpers the generated `run` calls; each prints one comparable line.
const JET_HARNESS: &str = r#"
fn conformance_encode(schema: String, sections: [RecordSection]) {
    encoded :: record_encode(schema, sections)
    bytes :: encoded.bytes ?? {
        print("encode problem: {encoded.problem ?? ""}")
        return
    }
    print("encode {mir_sha256_hex_bytes(bytes)}")
}

fn conformance_decode(bytes: [U8]) {
    decoded :: record_decode(bytes)
    record :: decoded.record ?? {
        print("decode {decoded.problem ?? ""}")
        return
    }
    again :: record_encode(record.schema, record.sections).bytes ?? [U8]{}
    print("decode ok {mir_sha256_hex_bytes(again)} {mir_sha256_hex_bytes(record.digest)}")
}
"#;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn jet_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn jet_bytes(bytes: &[u8]) -> String {
    let items = bytes.iter().map(u8::to_string).collect::<Vec<_>>();
    format!("[U8]{{{}}}", items.join(", "))
}

fn jet_int(value: i64) -> String {
    if value >= 0 {
        value.to_string()
    } else {
        format!("(0 - {})", value.unsigned_abs())
    }
}

/// A Jet literal for `value`. Map fields are written in reverse order so the
/// Jet encoder's own canonical sort is exercised.
fn jet_value(value: &RecordValue) -> String {
    match value {
        RecordValue::Null => "RecordValue.Null".to_string(),
        RecordValue::Bool(flag) => format!("RecordValue.Bool{{value: {flag}}}"),
        RecordValue::Int(number) => format!("RecordValue.Int{{value: {}}}", jet_int(*number)),
        RecordValue::Str(text) => format!("RecordValue.Str{{value: {}}}", jet_string(text)),
        RecordValue::Bytes(bytes) => format!("RecordValue.Bytes{{value: {}}}", jet_bytes(bytes)),
        RecordValue::List(values) => format!(
            "RecordValue.List{{values: [RecordValue]{{{}}}}}",
            values.iter().map(jet_value).collect::<Vec<_>>().join(", ")
        ),
        RecordValue::Map(fields) => format!(
            "RecordValue.Map{{fields: [RecordField]{{{}}}}}",
            fields
                .iter()
                .rev()
                .map(|(name, value)| format!("RecordField{{name: {}, value: {}}}", jet_string(name), jet_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        RecordValue::Digest(digest) => format!("RecordValue.Digest{{value: {}}}", jet_bytes(digest)),
    }
}

fn jet_sections(sections: &[RecordSection]) -> String {
    let rendered = sections
        .iter()
        .map(|section| {
            format!(
                "RecordSection{{id: {}, elements: [RecordValue]{{{}}}}}",
                section.tag,
                section.elements.iter().map(jet_value).collect::<Vec<_>>().join(", ")
            )
        })
        .collect::<Vec<_>>();
    format!("[RecordSection]{{{}}}", rendered.join(", "))
}

fn map(fields: &[(&str, RecordValue)]) -> RecordValue {
    RecordValue::Map(
        fields
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// Records both codecs must encode to identical bytes. Sections are listed
/// out of order on purpose; both encoders sort them.
fn record_fixtures() -> Vec<(&'static str, Vec<RecordSection>)> {
    let digest = std::array::from_fn::<u8, 32, _>(|index| (index * 7) as u8);
    let long_text = "é".repeat(90);
    vec![
        ("jet.program/v1", Vec::new()),
        (
            "jet.iface/v1",
            vec![
                RecordSection::new(
                    9,
                    vec![RecordValue::Digest(digest), RecordValue::Bytes(vec![0, 255, 128]), RecordValue::Bytes(Vec::new())],
                ),
                RecordSection::new(
                    1,
                    vec![
                        map(&[
                            ("zeta", RecordValue::Int(-1)),
                            ("alpha", RecordValue::Str("héllo".to_string())),
                            (
                                "mid",
                                RecordValue::List(vec![
                                    RecordValue::Null,
                                    RecordValue::Bool(true),
                                    RecordValue::Bool(false),
                                    RecordValue::Str("alpha".to_string()),
                                ]),
                            ),
                        ]),
                        RecordValue::Int(i64::MAX),
                        RecordValue::Int(i64::MIN),
                        RecordValue::Int(300),
                        RecordValue::Int(0),
                    ],
                ),
            ],
        ),
        (
            "jet.diags/v1",
            vec![
                RecordSection::new(
                    300,
                    (0..130).map(|index| RecordValue::Int(index - 65)).collect(),
                ),
                RecordSection::new(
                    2,
                    vec![
                        // Byte order, not case or length order: "B" < "a" < "ab" < "b" < "é".
                        map(&[
                            ("b", RecordValue::Null),
                            ("ab", RecordValue::List(Vec::new())),
                            ("a", map(&[])),
                            ("B", RecordValue::Str(String::new())),
                            ("é", RecordValue::Str(long_text.clone())),
                            ("", RecordValue::Str("{braces} \"quoted\" \\slash".to_string())),
                        ]),
                        RecordValue::Str(long_text),
                        RecordValue::List(vec![map(&[("k", map(&[("k", RecordValue::Str("k".to_string()))]))])]),
                    ],
                ),
            ],
        ),
    ]
}

/// A record body followed by its checksum, so decoding reaches the framing
/// checks after the checksum check.
fn sealed(body: &[u8]) -> Vec<u8> {
    let mut bytes = body.to_vec();
    bytes.extend_from_slice(&sha256(body));
    bytes
}

/// A body with schema "s", the given string table, and one section 1 holding
/// the given raw elements.
fn body_with(strings: &[&str], elements: &[Vec<u8>]) -> Vec<u8> {
    let mut body = b"JETR\x01\x01s".to_vec();
    body.push(strings.len() as u8);
    for text in strings {
        body.push(text.len() as u8);
        body.extend_from_slice(text.as_bytes());
    }
    body.extend_from_slice(&[1, 1, elements.len() as u8]);
    for element in elements {
        body.push(element.len() as u8);
    }
    for element in elements {
        body.extend_from_slice(element);
    }
    body
}

/// Byte strings both decoders must treat identically: valid records and one
/// of every rejection.
fn decode_fixtures() -> Vec<Vec<u8>> {
    let valid = encode_record("jet.items/v1", &record_fixtures()[1].1).expect("fixture encodes");
    let mut flipped = valid.clone();
    *flipped.last_mut().unwrap() ^= 1;
    let mut bad_magic = valid.clone();
    bad_magic[3] = b'X';
    let mut trailing = body_with(&["a"], &[vec![4, 0]]);
    trailing.push(0);
    vec![
        valid.clone(),
        encode_record("jet.program/v1", &[]).expect("empty record encodes"),
        valid[..20].to_vec(),
        flipped,
        bad_magic,
        sealed(b"JETR\x02\x01s\x00\x00"),
        sealed(b"JETR\x81\x00\x01s\x00\x00"),
        sealed(b"JETR\x01\x01s\x00\x02\x01\x00\x01\x00"),
        sealed(b"JETR\x01\x01s\x01\x01\xff\x00"),
        sealed(&body_with(&["b", "a"], &[vec![7, 2, 0, 0, 1, 0]])),
        sealed(&body_with(&["a"], &[vec![7, 2, 0, 0, 0, 0]])),
        sealed(&body_with(&[], &[vec![4, 5]])),
        sealed(&body_with(&[], &[vec![9]])),
        sealed(&body_with(&[], &[vec![0, 0]])),
        sealed(&body_with(&[], &[vec![5, 3, 1]])),
        sealed(&body_with(&[], &[vec![3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02]])),
        sealed(&trailing),
    ]
}

fn rust_decode_line(bytes: &[u8]) -> String {
    match decode_record(bytes) {
        Ok(record) => {
            let again = encode_record(&record.schema, &record.sections).expect("decoded record re-encodes");
            format!("decode ok {} {}", hex(&again), hex(&record.digest))
        }
        Err(error) => format!("decode {error}"),
    }
}

struct KeyFixture {
    files: Vec<(&'static str, &'static str)>,
    manifest: Option<&'static str>,
    facts: Vec<(&'static str, &'static str)>,
    compiler: &'static str,
    package: &'static str,
    dependencies: Vec<(&'static str, &'static str)>,
}

fn key_fixtures() -> Vec<KeyFixture> {
    vec![
        KeyFixture {
            files: Vec::new(),
            manifest: None,
            facts: Vec::new(),
            compiler: "",
            package: "single-file:main.jet",
            dependencies: Vec::new(),
        },
        KeyFixture {
            files: vec![
                ("Source/b.jet", "fn b() {}\n"),
                ("Source/A.jet", "// é\nfn a() {}\n"),
                ("Source/a/z.jet", ""),
            ],
            manifest: Some("package jet_sema {\n    version: \"0.1.0\"\n}\n"),
            facts: vec![("web", "false"), ("active_os", "linux"), ("edition", "2026")],
            compiler: "jet-dev-3f2a",
            package: "jet_sema@0.1.0",
            dependencies: vec![
                ("jet_parser@0.1.0", "b1946ac92492d2347c6235b4d2611184e6b1b3f7c4b5c1f8b1946ac92492d234"),
                ("core", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ],
        },
    ]
}

#[test]
fn jet_record_codec_and_package_keys_match_rust_bytes() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut program = String::new();
    for source in JET_SOURCES {
        program.push_str(&fs::read_to_string(repo.join(source)).expect("read Jet codec source"));
        program.push('\n');
    }
    program.push_str(JET_HARNESS);
    program.push_str("\nfn run() {\n");
    let mut expected = Vec::new();

    for (schema, sections) in record_fixtures() {
        let _ = writeln!(program, "    conformance_encode({}, {})", jet_string(schema), jet_sections(&sections));
        let bytes = encode_record(schema, &sections).expect("fixture encodes");
        expected.push(format!("encode {}", hex(&bytes)));
    }
    for bytes in decode_fixtures() {
        let _ = writeln!(program, "    conformance_decode({})", jet_bytes(&bytes));
        expected.push(rust_decode_line(&bytes));
    }
    for (index, fixture) in key_fixtures().into_iter().enumerate() {
        let files = fixture
            .files
            .iter()
            .map(|(path, source)| format!("PackageKeySourceFile{{relative_path: {}, source: {}}}", jet_string(path), jet_string(source)))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = fixture.manifest.map_or("None".to_string(), |text| format!("Val({})", jet_string(text)));
        let facts = fixture
            .facts
            .iter()
            .map(|(name, value)| format!("PackageKeyTargetFact{{name: {}, value: {}}}", jet_string(name), jet_string(value)))
            .collect::<Vec<_>>()
            .join(", ");
        let dependencies = fixture
            .dependencies
            .iter()
            .map(|(identity, digest)| format!("PackageKeyDependency{{identity: {}, interface_digest: {}}}", jet_string(identity), jet_string(digest)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(program, "    source_{index} :: package_key_source_digest([PackageKeySourceFile]{{{files}}}, {manifest})");
        let _ = writeln!(program, "    facts_{index} :: package_key_target_facts_digest([PackageKeyTargetFact]{{{facts}}})");
        let _ = writeln!(
            program,
            "    key_{index} :: package_key_check_key({}, facts_{index}, {}, source_{index}, [PackageKeyDependency]{{{dependencies}}})",
            jet_string(fixture.compiler),
            jet_string(fixture.package),
        );
        let _ = writeln!(program, "    print(\"keys {{source_{index}}} {{facts_{index}}} {{key_{index}}}\")");

        let files = fixture
            .files
            .iter()
            .map(|(path, source)| (path.to_string(), source.as_bytes()))
            .collect::<Vec<_>>();
        let source = source_digest(&files, fixture.manifest.map(str::as_bytes));
        let facts = target_facts_digest(
            &fixture.facts.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect::<Vec<_>>(),
        );
        let key = package_check_key(
            fixture.compiler,
            &facts,
            fixture.package,
            &source,
            &fixture
                .dependencies
                .iter()
                .map(|(identity, digest)| (identity.to_string(), digest.to_string()))
                .collect::<Vec<_>>(),
        );
        expected.push(format!("keys {source} {facts} {key}"));
    }
    program.push_str("}\n");

    let scratch = Scratch::new("record-conformance");
    fs::write(scratch.join("main.jet"), &program).expect("write conformance program");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "main.jet"])
        .current_dir(&scratch.path)
        .env("JET_STORE_DIR", scratch.join("store"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run the Jet record codec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "Jet record codec failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = stdout.lines().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len(), "Jet printed a different number of lines:\n{stdout}");
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "line {index} differs between the Jet and Rust codecs");
    }
}
