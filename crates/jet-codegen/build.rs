// Bootstrap-only producer/host admission seam; dies at Jet-hosted cutover.
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(jet_parent_runtime_admission)");
    // Prelude/Observe.rs (included by src/lib.rs) leaves out the generated
    // runtime's C-ABI seed exports under this cfg, as jet_evalhost does for Mem.rs.
    println!("cargo:rustc-check-cfg=cfg(jet_codegen_host)");
    println!("cargo:rustc-cfg=jet_codegen_host");
    println!("cargo:rerun-if-env-changed=JET_PARENT_RUNTIME_ADMISSION_FILE");
    let Some(input) = env::var_os("JET_PARENT_RUNTIME_ADMISSION_FILE") else {
        return;
    };
    let input = PathBuf::from(input);
    println!("cargo:rerun-if-changed={}", input.display());
    let source =
        fs::read(&input).expect("read actual producer's private runtime admission constants");
    assert!(
        source.starts_with(
            b"// Generated from the same sealed runtime candidate as the installed .so.\n"
        ),
        "host admission input must be the producer's generated source"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(output.join("parent-runtime-admission.rs"), source)
        .expect("embed private runtime admission constants");
    println!("cargo:rustc-cfg=jet_parent_runtime_admission");
}
