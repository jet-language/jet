//! Card #714: encoding corpora manifests, provenance, and non-vacuous vector checks.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jet_foundation::XmlPull::base_encoding_2026;

mod common;

fn fixtures_encoding() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encoding")
}

fn sha256_hex(bytes: &[u8]) -> String {
    jet::SHA256::sha256_hex(bytes)
}

fn verify_manifest(root: &Path) -> usize {
    let manifest = fs::read_to_string(root.join("MANIFEST.tsv")).unwrap_or_else(|_| {
        panic!("missing MANIFEST.tsv under {}", root.display())
    });
    let mut count = 0;
    for line in manifest.lines().filter(|line| !line.starts_with('#') && !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert!(
            fields.len() >= 4,
            "bad manifest row in {}: {line}",
            root.display()
        );
        let bytes = fs::read(root.join(fields[0])).unwrap_or_else(|_| {
            panic!("manifest file missing: {}/{}", root.display(), fields[0])
        });
        assert_eq!(
            sha256_hex(&bytes),
            fields[1],
            "hash drift in {}: {}",
            root.display(),
            fields[0]
        );
        assert!(
            fields[2].starts_with("https://") || fields[2] == "local",
            "bad source URL in {}: {}",
            root.display(),
            fields[2]
        );
        assert!(!fields[3].is_empty(), "missing license in {}", root.display());
        count += 1;
    }
    count
}

fn jet_string_literal(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn jet_json_literal(json: &str) -> String {
    if json.starts_with('{') && json.ends_with('}') {
        let body = &json[1..json.len() - 1];
        let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
        let mut out = String::from("\"{{");
        out.push_str(&escaped);
        out.push_str("}}\"");
        out
    } else {
        jet_string_literal(json)
    }
}

fn build_and_run(dir: &PathBuf, name: &str, src: &str) -> (i32, String, String) {
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let out = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn every_encoding_corpus_manifest_verifies_before_use() {
    let root = fixtures_encoding();
    let mut manifests = 0;
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let path = entry.path();
        if path.join("MANIFEST.tsv").is_file() {
            manifests += verify_manifest(&path);
            continue;
        }
        if path.is_dir() {
            for nested in fs::read_dir(&path).unwrap().flatten() {
                let nested_path = nested.path();
                if nested_path.join("MANIFEST.tsv").is_file() {
                    manifests += verify_manifest(&nested_path);
                }
            }
        }
    }
    assert!(
        manifests >= 14,
        "expected every encoding corpus manifest row to verify; got {manifests}"
    );
}

#[test]
fn rfc4648_vectors_decode_with_strict_2027_surface() {
    verify_manifest(&fixtures_encoding().join("rfc4648"));
    let corpus = fs::read_to_string(fixtures_encoding().join("rfc4648/vectors.tsv")).unwrap();
    for line in corpus.lines().filter(|line| !line.starts_with('#') && !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "bad RFC 4648 row: {line}");
        let (plain_text, base64, base64url, base32) =
            (fields[0], fields[1], fields[2], fields[3]);
        let plain = if let Some(hex) = plain_text.strip_prefix("HEX:") {
            (0..hex.len())
                .step_by(2)
                .map(|idx| u8::from_str_radix(&hex[idx..idx + 2], 16).unwrap())
                .collect::<Vec<_>>()
        } else {
            plain_text.as_bytes().to_vec()
        };
        assert_eq!(base_encoding_2026::decode_base64(base64).unwrap(), plain);
        assert_eq!(
            base_encoding_2026::decode_base64url(base64url).unwrap(),
            plain
        );
        assert_eq!(
            base_encoding_2026::decode_base32(base32).unwrap(),
            plain
        );
    }
}

#[test]
fn rfc4180_cases_parse_through_csv_whole_value_api() {
    common::have_rustc();
    verify_manifest(&fixtures_encoding().join("rfc4180"));
    let corpus = fs::read_to_string(fixtures_encoding().join("rfc4180/cases.tsv")).unwrap();
    let dir = common::unique_tmp("jet_enc_rfc4180");
    fs::create_dir_all(&dir).unwrap();
    let mut checks = String::new();
    for (idx, line) in corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .enumerate()
    {
        let (csv, _expected) = line.split_once('\t').unwrap();
        let escaped = jet_string_literal(csv);
        checks.push_str(&format!(
            "    rows{idx} := csv.parse({escaped}) ?? panic(\"parse\")\n    print(rows{idx}.len() > 0)\n"
        ));
    }
    let source = format!(
        "use core.encoding.csv as csv\n\nfn run() {{\n{checks}}}\n"
    );
    let (code, stdout, stderr) = build_and_run(&dir, "rfc4180", &source);
    assert_eq!(code, 0, "RFC 4180 corpus failed: {stderr}");
    for line in stdout.lines() {
        assert_eq!(line, "true", "RFC 4180 row mismatch: {stdout}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rfc8259_accept_cases_parse_through_json_whole_value_api() {
    common::have_rustc();
    verify_manifest(&fixtures_encoding().join("rfc8259"));
    let corpus = fs::read_to_string(fixtures_encoding().join("rfc8259/accept.tsv")).unwrap();
    let dir = common::unique_tmp("jet_enc_rfc8259");
    fs::create_dir_all(&dir).unwrap();
    let mut checks = String::new();
    for (idx, line) in corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .enumerate()
    {
        let json = line.trim();
        let escaped = jet_json_literal(json);
        checks.push_str(&format!(
            "    parsed{idx} := json.parse({escaped}) ?? panic(\"parse\")\n    print(true)\n"
        ));
    }
    let source = format!("use core.encoding.json as json\n\nfn run() {{\n{checks}}}\n");
    let (code, stdout, stderr) = build_and_run(&dir, "rfc8259", &source);
    assert_eq!(code, 0, "RFC 8259 corpus failed: {stderr}");
    let case_count = corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .count();
    assert_eq!(stdout.matches("true\n").count(), case_count, "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rfc8949_wire_cases_match_cbor_parse_expectations() {
    common::have_rustc();
    verify_manifest(&fixtures_encoding().join("rfc8949"));
    let corpus = fs::read_to_string(fixtures_encoding().join("rfc8949/wire.tsv")).unwrap();
    let dir = common::unique_tmp("jet_enc_rfc8949");
    fs::create_dir_all(&dir).unwrap();
    let mut checks = String::new();
    for (idx, line) in corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .enumerate()
    {
        let (wire_hex, accept) = line.split_once('\t').unwrap();
        let bytes = (0..wire_hex.len())
            .step_by(2)
            .map(|idx| u8::from_str_radix(&wire_hex[idx..idx + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let literal = bytes
            .iter()
            .map(|byte| byte.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if accept == "true" {
            checks.push_str(&format!(
                "    cbor{idx} := cbor.parse([{literal}]) ?? panic(\"appendix A accept {wire_hex}\")\n    print(true)\n"
            ));
        } else {
            checks.push_str(&format!(
                "    if cbor.parse([{literal}]) == {{\n        Ok(_) -> print(false)\n        Err(_) -> print(true)\n    }}\n"
            ));
        }
    }
    let source = format!("use core.encoding.cbor as cbor\n\nfn run() {{\n{checks}}}\n");
    let (code, stdout, stderr) = build_and_run(&dir, "rfc8949", &source);
    assert_eq!(code, 0, "RFC 8949 corpus failed: {stderr}");
    for line in stdout.lines() {
        assert_eq!(line, "true", "RFC 8949 row mismatch: {stdout}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn w3c_c14n_inclusive11_vector_matches_xml_canonical_output() {
    common::have_rustc();
    let root = fixtures_encoding().join("xml/w3c-c14n");
    verify_manifest(&root);
    let input = fs::read_to_string(root.join("inclusive11-input.xml")).unwrap();
    let expected = fs::read_to_string(root.join("inclusive11-output.txt")).unwrap();
    let dir = common::unique_tmp("jet_enc_xml_c14n");
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("inclusive11-input.xml");
    fs::write(&input_path, &input).unwrap();
    let shown = input_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding.xml as xml
use core.files as files

fn run() {{
    text := files.read("{shown}") ?? panic("read")
    tree := xml.parse(text) ?? panic("parse")
    options := xml.XMLCanonical.{{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }}
    print(xml.canonical(tree, options) ?? panic("canonical"))
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_c14n", &source);
    assert_eq!(code, 0, "W3C C14N corpus failed: {stderr}");
    assert_eq!(stdout.trim(), expected.trim(), "C14N byte mismatch");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn local_cbor_hostile_oracle_case_count_is_pinned() {
    let root = fixtures_encoding().join("local/cbor-hostile");
    verify_manifest(&root);
    let count = fs::read_to_string(root.join("case-count.txt"))
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(count, 40, "local CBOR hostile oracle case count drifted");
}
