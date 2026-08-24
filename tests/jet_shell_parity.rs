//! The Jet-native shell declarations stay aligned with the checked-in Nix
//! oracle. The no-Nix manifest comparison lives in
//! `scripts/agent/verify-jet-shell-parity.js`; this test proves both selected
//! environment modules also pass the typed evaluator.

use jet_env_model::ModuleEval;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn package_set(plan: &ModuleEval::EnvPlan) -> BTreeSet<String> {
    plan.package_refs.iter().cloned().collect()
}

#[test]
fn default_and_full_shell_declarations_are_typed_and_complete() {
    let root = project_root();
    let source = fs::read_to_string(root.join("env.jet")).expect("root env.jet");
    assert!(!source.contains("/nix/store"));

    let default = ModuleEval::evaluate_env_with_environment(&source, &root, Some("dev"))
        .expect("default environment evaluates");
    let expected_default = [
        "cargo",
        "sccache",
        "clippy",
        "rustc",
        "rustfmt",
        "gcc",
        "clang",
        "lld",
        "nodejs_22",
        "python3",
        "nixfmt",
        "ripgrep",
        "jq",
        "gh",
        "fd",
        "bashInteractive",
        "zsh",
        "fish",
        "util-linux",
        "wasm-tools",
        "tree-sitter",
        "pkg-config",
        "tzdata",
        "vulkan-loader",
        "ruby",
        "php",
        "rWrapper",
        "rPackages.jsonlite",
    ]
    .into_iter()
    .map(|name| format!("{name}@default"))
    .collect::<BTreeSet<_>>();
    assert_eq!(package_set(&default), expected_default);

    let full = ModuleEval::evaluate_env_with_environment(&source, &root, Some("full"))
        .expect("full environment evaluates");
    assert_eq!(full.package_refs.len(), 51);
    for name in [
        "rustup",
        "gnat",
        "fpc",
        "dart",
        "powershell",
        "gfortran",
        "gnucobol",
        "go",
        "jdk",
        "dotnet-sdk_8",
        "tcl",
        "lua5_4",
        "octave",
        "qemu",
        "wasmtime",
        "emscripten",
        "lldb",
        "raylib",
        "chromium",
        "firefox",
        "geckodriver",
        "gtk4",
        "bubblewrap",
        "rWrapper",
        "rPackages.jsonlite",
        "tzdata",
        "vulkan-loader",
    ] {
        assert!(
            package_set(&full).contains(&format!("{name}@default")),
            "full environment misses {name}"
        );
    }
}
