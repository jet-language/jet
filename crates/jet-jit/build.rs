use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    write_yaml_std(&manifest);
    write_sketch_rt(&manifest);
}

fn write_yaml_std(manifest: &PathBuf) {
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

fn write_sketch_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/Core.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Prelude/Core.rs");
    let start = raw
        .find("// ── D-APPROX1=A: core.sketch")
        .expect("sketch marker in Core.rs");
    let after = &raw[start..];
    let end_rel = after
        .find("\nthread_local! {")
        .expect("thread_local after sketch in Core.rs");
    let mut body = after[..end_rel].to_string();
    // JetShow is AOT-prelude-only; strip those impls for the JIT include.
    while let Some(impl_at) = body.find("impl JetShow for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n}\n")
            .map(|i| i + 3)
            .expect("JetShow impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    // Pub the sketch types so JIT hosts can name them.
    for name in [
        "JetHyperLogLog",
        "JetTDigest",
        "JetCountMinSketch",
        "JetReservoirSampler",
        "JetReservoirInner",
    ] {
        let from = format!("struct {name}");
        let to = format!("pub(crate) struct {name}");
        body = body.replace(&from, &to);
    }
    // Methods must be crate-visible for JIT host shims outside this module.
    for method in ["new", "add", "count", "quantile", "sample"] {
        let from = format!("    fn {method}(");
        let to = format!("    pub(crate) fn {method}(");
        body = body.replace(&from, &to);
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("sketch_rt.rs");
    std::fs::write(&out, body).expect("write sketch_rt.rs");
}
