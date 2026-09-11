#[path = "src/SHA256.rs"]
mod source_sha256;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_HASH_PREFIX: &str = "// Source SHA-256: ";

fn workspace_file(manifest_dir: &Path, relative: &str) -> PathBuf {
    manifest_dir.join(relative)
}

fn source_hash(path: &Path) -> String {
    source_sha256::sha256_file_hex(path)
        .unwrap_or_else(|error| panic!("cannot hash generated-table source {}: {error}", path.display()))
}

fn generated_hash(path: &Path) -> String {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read generated table {}: {error}", path.display()));
    let mut matches = source
        .lines()
        .filter_map(|line| line.strip_prefix(SOURCE_HASH_PREFIX));
    let Some(hash) = matches.next() else {
        panic!("generated table {} has no source digest", path.display());
    };
    if matches.next().is_some() {
        panic!("generated table {} has multiple source digests", path.display());
    }
    hash.to_string()
}

fn check_generated_view(manifest_dir: &Path, source: &str, generated: &str) {
    let source_path = workspace_file(manifest_dir, source);
    let generated_path = workspace_file(manifest_dir, generated);
    let expected = source_hash(&source_path);
    let actual = generated_hash(&generated_path);
    if actual != expected {
        panic!(
            "generated table {} is stale for {}; run `node scripts/agent/gen-core-tables.mjs --write`",
            generated_path.display(),
            source_path.display(),
        );
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(jet_release)");
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for a crate build script"),
    );

    const EFFECT_SOURCE: &str = "../jet-codegen/src/Prelude/Effects.jet";
    const CORE_SOURCE: &str = "../jet-codegen/src/Prelude/Core.jet";
    for path in [EFFECT_SOURCE, CORE_SOURCE] {
        println!("cargo:rerun-if-changed={}", workspace_file(&manifest_dir, path).display());
    }
    for path in [
        "src/Effects.rs",
        "src/BuildEffects.rs",
        "src/CoreModuleExports.rs",
        "src/RingLayer.rs",
        "src/Syntax/core_calls.rs",
    ] {
        println!("cargo:rerun-if-changed={}", workspace_file(&manifest_dir, path).display());
    }

    check_generated_view(&manifest_dir, EFFECT_SOURCE, "src/Effects.rs");
    check_generated_view(&manifest_dir, EFFECT_SOURCE, "src/BuildEffects.rs");
    check_generated_view(&manifest_dir, CORE_SOURCE, "src/CoreModuleExports.rs");
    check_generated_view(&manifest_dir, CORE_SOURCE, "src/RingLayer.rs");
    check_generated_view(&manifest_dir, CORE_SOURCE, "src/Syntax/core_calls.rs");
}
