use std::{env, fs, path::PathBuf};
fn main() {
    let repo = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../../..");
    let path = env::var_os("JET_PLATFORM_HOST_CARRIERS").map(PathBuf::from)
        .unwrap_or_else(|| repo.join("crates/jet-codegen/src/Prelude/Core/CAbiHost.rs"));
    println!("cargo:rerun-if-env-changed=JET_PLATFORM_HOST_CARRIERS");
    println!("cargo:rerun-if-changed={}", path.display());
    let input = fs::read_to_string(&path).expect("the composed epoch must provide generated CAbiHost.rs");
    let constants = input.lines().filter(|line| line.contains("pub(crate) const HOST_IO_KIND_"))
        .collect::<Vec<_>>().join("\n");
    assert!(!constants.is_empty(), "regenerate host carriers at the canonical Core enum epoch");
    fs::write(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("host-io-kinds.rs"), constants).unwrap();
    println!("cargo:rustc-cfg=jet_platform_seeds");
}
