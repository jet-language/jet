//! Edition-gated encoding surfaces (card #712 / #715 C5).

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_enc_edition_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_single_file(root: &PathBuf, body: &str) -> PathBuf {
    let path = root.join("run.jet");
    fs::write(&path, body).unwrap();
    path
}

fn write_project(root: &PathBuf, edition: &str, body: &str) -> PathBuf {
    fs::write(
        root.join("package.jet"),
        format!("name: \"enc\"\nversion: \"0.1.0\"\nedition: \"{edition}\"\n"),
    )
    .unwrap();
    write_single_file(root, body)
}

#[test]
fn edition_2027_base64_strict_rejects_whitespace_without_allowance_surface() {
    // D-ENCBASE-STRICT1=A: edition 2027 strict decode rejects ASCII whitespace.
    // Named allowance args (`allow_whitespace`, `allow_missing_padding`) are
    // ratified but not yet on the public sema/codegen surface (fixed_sigs and
    // emit still 1-arg). That gap is explicit and cannot satisfy allowance parity.
    let root = scratch("2027_b64_strict");
    let path = write_project(
        &root,
        "2027",
        "use core.encoding.base64 as base64\n\nfn run() {\n    if base64.decode(\"Zg==\\n\") == {\n        .Ok(_) -> print(\"accepted\")\n        .Err(reason) -> print(reason)\n    }\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "edition 2027 1-arg decode must type-check:\n{}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );

    // Allowance arity is not shipped — keep the red explicit.
    let allow_root = scratch("2027_b64_allow_gap");
    let allow_path = write_project(
        &allow_root,
        "2027",
        "use core.encoding.base64 as base64\n\nfn run() {\n    bytes := base64.decode(\"Zg==\\n\", true, false) ?? panic(\"decode\")\n    print(bytes.len())\n}\n",
    );
    let allow_diags = jet::check_with_path(allow_path.to_str().unwrap());
    assert!(
        allow_diags.iter().any(|d| d.code == "E0104"),
        "named gap: 2027 allowance args must not silently type-check until wired; got:\n{}",
        jet::render_diagnostics(allow_path.to_str().unwrap(), "", &allow_diags)
    );
}

#[test]
fn edition_2026_base64_keeps_compatibility_union() {
    let root = scratch("2026_b64");
    let path = write_project(
        &root,
        "2026",
        "use core.encoding.base64 as base64\n\nfn run() {\n    bytes := base64.decode(\"Zg==\\n\") ?? panic(\"decode\")\n    print(bytes.len())\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "edition 2026 should keep compatibility decode:\n{}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );
}

#[test]
fn edition_2027_json_canonical_is_fallible_jcs() {
    let root = scratch("2027_json_canon");
    let path = write_project(
        &root,
        "2027",
        "use core.encoding.json as json\n\nfn run() {\n    data := json.parse(\"{{\\\"b\\\":2,\\\"a\\\":1}}\") ?? panic(\"json\")\n    print(json.canonical(data) ?? panic(\"canon\"))\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "edition 2027 json.canonical must type-check:\n{}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );
}

#[test]
fn edition_2026_json_canonical_stays_infallible() {
    let root = scratch("2026_json_canon");
    let path = write_project(
        &root,
        "2026",
        "use core.encoding.json as json\n\nfn run() {\n    data := json.parse(\"{{\\\"b\\\":2,\\\"a\\\":1}}\") ?? panic(\"json\")\n    print(json.canonical(data))\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "edition 2026 json.canonical must stay infallible:\n{}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );
}

#[test]
fn edition_2027_cbor_encode_emits_l2001() {
    let root = scratch("l2001");
    let path = write_project(
        &root,
        "2027",
        "use core.encoding.cbor as cbor\nuse core.encoding.json as json\n\nfn run() {\n    tree := json.parse(\"{{}}\") ?? panic(\"json\")\n    payload := cbor.encode(tree)\n    print(\"ok\")\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.iter().any(|d| d.code == "L2001"),
        "expected L2001 for cbor.encode, got: {}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );
}

#[test]
fn edition_2027_cbor_decode_emits_l2001_and_2028_removes_it() {
    let source = "use core.encoding.cbor as cbor\n\nfn run() {\n    decoded := cbor.decode([U8]{ 0x81, 0x01 }) ?? panic(\"decode\")\n    print(decoded)\n}\n";
    let before = scratch("2027_cbor_decode");
    let before_path = write_project(&before, "2027", source);
    let before_diags = jet::check_with_path(before_path.to_str().unwrap());
    assert!(
        before_diags
            .iter()
            .any(|diagnostic| diagnostic.code == "L2001"),
        "expected L2001 for cbor.decode, got: {}",
        jet::render_diagnostics(before_path.to_str().unwrap(), "", &before_diags)
    );

    let after = scratch("2028_cbor_decode");
    let after_path = write_project(&after, "2028", source);
    let after_diags = jet::check_with_path(after_path.to_str().unwrap());
    assert!(
        after_diags
            .iter()
            .any(|diagnostic| diagnostic.code == "E2002"),
        "expected E2002 after cbor.decode removal, got: {}",
        jet::render_diagnostics(after_path.to_str().unwrap(), "", &after_diags)
    );
}

#[test]
fn user_deprecated_marker_emits_l2001_for_a_consumer() {
    let root = scratch("user_deprecated");
    let path = write_project(
        &root,
        "2027",
        "#Deprecated(since: \"1.2\", use: \"parse\")\npub fn decode() {}\n\nfn run() {\n    decode()\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags
            .iter()
            .any(|diagnostic| diagnostic.code == "L2001" && diagnostic.what.contains("`decode`")),
        "expected L2001 for a user #Deprecated function, got: {}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
    );

    let removed_root = scratch("user_removed");
    let removed_path = write_project(
        &removed_root,
        "2028",
        "#Deprecated(since: \"2027\", use: \"parse\", removed_in: \"2028\")\npub fn decode() {}\n\nfn run() {\n    decode()\n}\n",
    );
    let removed_diags = jet::check_with_path(removed_path.to_str().unwrap());
    assert!(
        removed_diags
            .iter()
            .any(|diagnostic| diagnostic.code == "E2002" && diagnostic.what.contains("`decode`")),
        "expected E2002 after the user marker removal edition, got: {}",
        jet::render_diagnostics(removed_path.to_str().unwrap(), "", &removed_diags)
    );

    let before_root = scratch("user_removed_dormant");
    let before_path = write_project(
        &before_root,
        "2026",
        "#Deprecated(since: \"2027\", use: \"parse\", removed_in: \"2028\")\npub fn decode() {}\n\nfn run() {\n    decode()\n}\n",
    );
    let before_diags = jet::check_with_path(before_path.to_str().unwrap());
    assert!(
        before_diags
            .iter()
            .all(|diagnostic| diagnostic.code != "L2001" && diagnostic.code != "E2002"),
        "removed_in must stay dormant before the deprecation edition: {}",
        jet::render_diagnostics(before_path.to_str().unwrap(), "", &before_diags)
    );
}

#[test]
fn lifecycle_marker_metadata_erases_before_web_codegen() {
    let source = "#Deprecated(since: \"2027\", use: \"replacement\", removed_in: \"2028\")\npub fn legacy() { print(\"legacy\") }\npub fn replacement() { print(\"replacement\") }\n\nfn run() {\n    replacement()\n}\n";
    let output = jet::compile_web_with_path(source, "tests/fixtures/lifecycle_marker_web.jet")
        .expect("user lifecycle marker must compile for web");
    let web = output
        .web
        .expect("user lifecycle marker must produce web artifacts");
    assert!(!web.wasm_rust.contains("Deprecated"));
    assert!(!web.js_app.contains("Deprecated"));
}

#[test]
fn deprecation_fix_replaces_plain_and_core_member_names() {
    let root = scratch("deprecation_fix");
    let path = write_project(
        &root,
        "2027",
        "#Deprecated(since: \"2027\", use: \"parse\")\npub fn decode() {}\npub fn parse() {}\n\nfn run() {\n    decode()\n}\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", path.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "user deprecation fix failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(&path).unwrap().contains("    parse()"),
        "plain replacement was not applied"
    );

    let cbor_root = scratch("cbor_deprecation_fix");
    let cbor_path = write_project(
        &cbor_root,
        "2027",
        "use core.encoding.cbor as cbor\nuse core.encoding.json as json\n\nfn run() {\n    tree := json.parse(\"{{}}\") ?? panic(\"json\")\n    payload := cbor.encode(tree)\n    print(\"ok\")\n}\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fix", cbor_path.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Core deprecation fix failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(&cbor_path)
            .unwrap()
            .contains("payload := cbor.to_bytes(tree)"),
        "Core replacement was not applied"
    );
}
