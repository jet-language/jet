// The bridge's jet-foundation is a fixed snapshot: its table views (Effects,
// BuildEffects, CoreModuleExports, RingLayer, Syntax/core_calls) are not
// regenerated from the Jet sources, so the build checks none of them.
#[path = "src/SHA256.rs"]
mod source_sha256;
#[path = "build/MirImageCodec.rs"]
mod mir_image_codec;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(jet_release)");
    println!("cargo:rustc-check-cfg=cfg(jet_native_pack)");
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for a crate build script"),
    );
    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").expect("OUT_DIR is set for a crate build script"),
    );
    println!("cargo:rerun-if-changed=build/MirImageCodec.rs");
    mir_image_codec::generate(&manifest_dir, &out_dir);
}
