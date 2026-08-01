//! Real Component Model build-plugin host proof.

use jet_comptime::Comptime::Build::{
    decode_build_plugin_response, encode_build_plugin_request, ContentDigest,
    WasmComponentPluginSpec, BUILD_PLUGIN_API_VERSION,
};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn sibling_host_instantiates_component_and_decodes_contribution() {
    let root = std::env::temp_dir().join(format!(
        "jet-build-plugin-e2e-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let component = wat::parse_str(include_str!(
        "../fixtures/build_plugin/empty.wat"
    ))
    .unwrap();
    let component_path = root.join("empty.wasm");
    std::fs::write(&component_path, &component).unwrap();

    let component_digest = ContentDigest::from_bytes(&component).as_str().to_string();
    let manifest = format!(
        "name = \"e2e\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{component_digest}\"\ncapabilities = []\n"
    );
    let manifest_path = root.join("plugin.manifest");
    std::fs::write(&manifest_path, &manifest).unwrap();
    let manifest_digest = ContentDigest::from_bytes(manifest.as_bytes());
    let spec = WasmComponentPluginSpec::new(
        "e2e",
        "1.0.0",
        component_digest,
    );
    let request = encode_build_plugin_request(&spec);
    let mut child = Command::new(env!("CARGO_BIN_EXE_jetpack"))
        .args([
            jet_comptime::Comptime::Build::BUILD_PLUGIN_HOST_SUBCOMMAND,
            manifest_path.to_str().unwrap(),
            component_path.to_str().unwrap(),
            manifest_digest.as_str(),
            spec.component_digest.as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&request).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "build-plugin host failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contribution = decode_build_plugin_response(&output.stdout).unwrap();
    assert!(contribution.actions.is_empty());
    assert!(contribution.targets.is_empty());
    assert!(contribution.generated_modules.is_empty());
    let _ = std::fs::remove_dir_all(root);
}
