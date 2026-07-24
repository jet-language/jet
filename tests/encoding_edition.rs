//! Edition-gated encoding surfaces (card #712).

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
fn edition_2027_base64_requires_explicit_whitespace_allowance() {
    let root = scratch("2027_b64_allow");
    let path = write_project(
        &root,
        "2027",
        "use core.encoding.base64 as base64\n\nfn run() {\n    bytes := base64.decode(\"Zg==\\n\", true, false) ?? panic(\"decode\")\n    print(bytes.len())\n}\n",
    );
    let diags = jet::check_with_path(path.to_str().unwrap());
    assert!(
        diags.is_empty(),
        "edition 2027 should accept explicit whitespace allowance:\n{}",
        jet::render_diagnostics(path.to_str().unwrap(), "", &diags)
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
