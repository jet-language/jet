//! Real Component Model build-plugin host proof.

use jet_comptime::Comptime::Build::{
    with_packaged_plugin_runner, ActionSpec, BuildCapability, BuildContext, BuildPolicy,
    ContentDigest, PackagedPluginContribution, TargetKind, WasmComponentPluginSpec,
    BUILD_PLUGIN_API_VERSION, BUILD_PLUGIN_MAX_RESPONSE_BYTES,
};
use std::path::Path;

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn scalar(value: &str) -> String {
    hex(value.as_bytes())
}

fn list(values: &[&str]) -> String {
    let mut out = format!("{}:", values.len());
    for value in values {
        out.push_str(&format!("{}:{}", value.len(), hex(value.as_bytes())));
    }
    out
}

fn map(values: &[(&str, &str)]) -> String {
    let mut flat = Vec::with_capacity(values.len() * 2);
    for (key, value) in values {
        flat.push(*key);
        flat.push(*value);
    }
    list(&flat)
}

fn guest_response(invalid_path: bool) -> String {
    let output = if invalid_path {
        "../outside"
    } else {
        "build/plugin.out"
    };
    let action = [
        scalar("plugin-action"),
        list(&["src/input.jet"]),
        list(&[output]),
        list(&["plugin-tool", "src/input.jet"]),
        map(&[("MODE", "release")]),
        list(&["MODE"]),
        list(&["fs"]),
        scalar("cached"),
        scalar("generic"),
        String::new(),
        list(&[]),
        String::new(),
        map(&[("origin", "guest")]),
        map(&[("tool", "1.0")]),
        list(&["cpu"]),
        String::new(),
        String::new(),
    ]
    .join("\t");
    let target = [
        scalar("executable"),
        scalar("plugin-target"),
        list(&["src/input.jet"]),
        list(&[]),
        list(&[output]),
        list(&[]),
        list(&["plugin-action"]),
        list(&[]),
        String::new(),
        String::new(),
        map(&[("origin", "guest")]),
    ]
    .join("\t");
    format!(
        "version=1\nstatus=ok\nactions=1\ntargets=1\ngenerated=0\naction\t{action}\ntarget\t{target}\n"
    )
}

fn component_for(response: &str) -> Vec<u8> {
    let wat_response = response.replace('\n', "\\n").replace('\t', "\\09");
    let len = response.len();
    let wat = format!(
        "(component
          (core module $m
            (memory (export \"memory\") 80)
            (global $heap (mut i32) (i32.const 4096))
            (data (i32.const 1024) \"{wat_response}\")
            (func $realloc (export \"cabi_realloc\")
              (param $ptr i32) (param $old i32) (param $align i32) (param $new i32)
              (result i32)
              (local $p i32)
              (local.set $p (global.get $heap))
              (local.set $p (i32.and (i32.add (local.get $p) (i32.sub (local.get $align) (i32.const 1)))
                (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
              (global.set $heap (i32.add (local.get $p) (local.get $new)))
              (local.get $p))
            (func $build (export \"build\") (param i32 i32) (result i32)
              (local $ret i32)
              (local.set $ret (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 8)))
              (i32.store (local.get $ret) (i32.const 1024))
              (i32.store offset=4 (local.get $ret) (i32.const {len}))
              (local.get $ret)))
          (core instance $i (instantiate $m))
          (type $t (func (param \"request\" (list u8)) (result (list u8))))
          (func $build (type $t)
            (canon lift (core func $i \"build\") (memory $i \"memory\") (realloc (func $i \"cabi_realloc\")))
          )
          (export \"build\" (func $build)))"
    );
    wat::parse_str(wat).unwrap()
}

fn production_runner(
    manifest_path: &Path,
    component_path: &Path,
    spec: &WasmComponentPluginSpec,
    manifest_digest: &str,
) -> Result<PackagedPluginContribution, String> {
    jet_driver::BuildPluginHook::run_packaged_build_plugin(
        manifest_path,
        component_path,
        spec,
        manifest_digest,
    )
}

#[test]
fn sibling_host_instantiates_component_and_applies_guest_graph() {
    let root = std::env::temp_dir().join(format!(
        "jet-build-plugin-e2e-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let component = component_for(&guest_response(false));
    let component_path = root.join("empty.wasm");
    std::fs::write(&component_path, &component).unwrap();

    let component_digest = ContentDigest::from_bytes(&component).as_str().to_string();
    let manifest = format!(
        "name = \"e2e\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{component_digest}\"\ncapabilities = [\"fs\"]\n"
    );
    let manifest_path = root.join("plugin.manifest");
    std::fs::write(&manifest_path, &manifest).unwrap();
    let manifest_digest = ContentDigest::from_bytes(manifest.as_bytes());
    let spec = WasmComponentPluginSpec::new(
        "e2e",
        "1.0.0",
        component_digest.clone(),
    )
    .with_capability(BuildCapability::FS);
    let contribution = production_runner(
        &manifest_path,
        &component_path,
        &spec,
        manifest_digest.as_str(),
    )
    .unwrap();
    assert_eq!(contribution.actions.len(), 1);
    assert_eq!(contribution.targets.len(), 1);

    let mut context = BuildContext::new();
    let policy = BuildPolicy::allow_all().with_plugin_grant("e2e", BuildCapability::FS);
    let applied = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &manifest_path,
            &component_path,
            &policy,
        )
    })
    .unwrap();
    assert_eq!(applied.actions.len(), 1);
    let plan = context.plan().unwrap();
    assert_eq!(plan.actions()[0].name, "plugin-action");
    assert_eq!(plan.actions()[0].outputs[0].as_str(), "build/plugin.out");
    assert_eq!(plan.targets()[0].kind, TargetKind::Executable);
    assert_eq!(plan.plugins()[0].name, "e2e");
    assert_eq!(plan.plugins()[0].component_digest, component_digest);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn packaged_host_rejects_invalid_graph_and_traps_guest() {
    let root = std::env::temp_dir().join(format!(
        "jet-build-plugin-hostile-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let response = guest_response(true);
    let component = component_for(&response);
    let component_path = root.join("hostile.wasm");
    std::fs::write(&component_path, &component).unwrap();
    let digest = ContentDigest::from_bytes(&component).as_str().to_string();
    let manifest = format!(
        "name = \"hostile\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{digest}\"\ncapabilities = [\"fs\"]\n"
    );
    let manifest_path = root.join("plugin.manifest");
    std::fs::write(&manifest_path, &manifest).unwrap();
    let mut context = BuildContext::new();
    context
        .action(
            "baseline",
            ActionSpec::cached(["baseline"]).with_outputs(["baseline.out"]),
        )
        .unwrap();
    let baseline = context.plan().unwrap();
    let policy = BuildPolicy::allow_all().with_plugin_grant("hostile", BuildCapability::FS);
    let error = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &manifest_path,
            &component_path,
            &policy,
        )
    })
    .unwrap_err();
    assert!(format!("{error:?}").contains("InvalidPath"));
    assert_eq!(context.plan().unwrap(), baseline);

    let trap_wat = include_str!("../fixtures/build_plugin/trap.wat");
    let trap = wat::parse_str(trap_wat).unwrap();
    let trap_path = root.join("trap.wasm");
    std::fs::write(&trap_path, &trap).unwrap();
    let trap_digest = ContentDigest::from_bytes(&trap).as_str().to_string();
    let trap_manifest = format!(
        "name = \"trap\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{trap_digest}\"\ncapabilities = []\n"
    );
    let trap_manifest_path = root.join("trap.manifest");
    std::fs::write(&trap_manifest_path, &trap_manifest).unwrap();
    let trap_error = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &trap_manifest_path,
            &trap_path,
            &BuildPolicy::allow_all(),
        )
    })
    .unwrap_err();
    let trap_error = format!("{trap_error:?}");
    assert!(trap_error.contains("trapped") || trap_error.contains("timed out"));
    assert_eq!(context.plan().unwrap(), baseline);

    let malformed_component = component_for("not-a-build-plugin-response");
    let malformed_path = root.join("malformed.wasm");
    std::fs::write(&malformed_path, &malformed_component).unwrap();
    let malformed_digest = ContentDigest::from_bytes(&malformed_component)
        .as_str()
        .to_string();
    let malformed_manifest = format!(
        "name = \"malformed\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{malformed_digest}\"\ncapabilities = []\n"
    );
    let malformed_manifest_path = root.join("malformed.manifest");
    std::fs::write(&malformed_manifest_path, &malformed_manifest).unwrap();
    let malformed_error = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &malformed_manifest_path,
            &malformed_path,
            &BuildPolicy::allow_all(),
        )
    })
    .unwrap_err();
    let malformed_error = format!("{malformed_error:?}");
    assert!(
        malformed_error.contains("version") || malformed_error.contains("response"),
        "{malformed_error}"
    );
    assert_eq!(context.plan().unwrap(), baseline);

    let oversized_response = "x".repeat(BUILD_PLUGIN_MAX_RESPONSE_BYTES + 1);
    let oversized_component = component_for(&oversized_response);
    let oversized_path = root.join("oversized.wasm");
    std::fs::write(&oversized_path, &oversized_component).unwrap();
    let oversized_digest = ContentDigest::from_bytes(&oversized_component)
        .as_str()
        .to_string();
    let oversized_manifest = format!(
        "name = \"oversized\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{oversized_digest}\"\ncapabilities = []\n"
    );
    let oversized_manifest_path = root.join("oversized.manifest");
    std::fs::write(&oversized_manifest_path, &oversized_manifest).unwrap();
    let oversized_error = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &oversized_manifest_path,
            &oversized_path,
            &BuildPolicy::allow_all(),
        )
    })
    .unwrap_err();
    let oversized_error = format!("{oversized_error:?}");
    assert!(oversized_error.contains("exceeds"), "{oversized_error}");
    assert_eq!(context.plan().unwrap(), baseline);

    let timeout_wat = include_str!("../fixtures/build_plugin/timeout.wat");
    let timeout = wat::parse_str(timeout_wat).unwrap();
    let timeout_path = root.join("timeout.wasm");
    std::fs::write(&timeout_path, &timeout).unwrap();
    let timeout_digest = ContentDigest::from_bytes(&timeout).as_str().to_string();
    let timeout_manifest = format!(
        "name = \"timeout\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{timeout_digest}\"\ncapabilities = []\n"
    );
    let timeout_manifest_path = root.join("timeout.manifest");
    std::fs::write(&timeout_manifest_path, &timeout_manifest).unwrap();
    let timeout_error = with_packaged_plugin_runner(production_runner, || {
        context.apply_packaged_wasm_component_plugin_from_host(
            &timeout_manifest_path,
            &timeout_path,
            &BuildPolicy::allow_all(),
        )
    })
    .unwrap_err();
    let timeout_error = format!("{timeout_error:?}");
    assert!(
        timeout_error.contains("timed out") || timeout_error.contains("trapped"),
        "{timeout_error}"
    );
    assert_eq!(context.plan().unwrap(), baseline);
    let _ = std::fs::remove_dir_all(root);
}
