use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/Yaml.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read JetStd/Yaml.rs");
    // Yaml.rs ends with an extra `}` for corelib string-concat embedding.
    let trimmed = {
        let t = raw.trim_end();
        let without = t.strip_suffix('}').expect("Yaml.rs trailing }");
        let without = without.trim_end();
        // Keep a trailing newline for include!
        format!("{without}\n")
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("yaml_std.rs");
    std::fs::write(&out, trimmed).expect("write yaml_std.rs");
}
