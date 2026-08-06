//! Edition-gated encoding surfaces (card #712 / #715 C5).

use std::fs;
use std::path::PathBuf;

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
        root.join("pkg.jet"),
        format!(
            "payload: {{ name: \"enc\", version: \"0.1.0\", edition: \"{edition}\" }}\n"
        ),
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
