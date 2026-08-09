//! D-ONCE-LAW1 guard for Core semantics shared with comptime.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn dispatched_namespaces(source: &str) -> BTreeSet<String> {
    source
        .match_indices("(\"core.")
        .filter_map(|(start, _)| {
            let namespace = &source[start + 2..];
            let end = namespace.find('"')?;
            Some(namespace[..end].trim_end_matches('.').to_string())
        })
        .collect()
}

#[test]
fn architecture_classifies_every_comptime_core_namespace() {
    let mut namespaces = dispatched_namespaces(&read(
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
    ));
    namespaces.extend(dispatched_namespaces(&read(
        "crates/jet-comptime/src/Comptime/CorePureParity.rs",
    )));
    // Solver construction enters through type-method dispatch rather than a
    // literal namespace arm, but `core.solve` is still part of this registry.
    namespaces.insert("core.solve".to_string());

    let architecture = read("docs/spec/architecture.md");
    for namespace in namespaces {
        assert!(
            architecture.contains(&format!("| `{namespace}` |")),
            "architecture classification is missing {namespace}"
        );
    }
}

#[test]
fn mime_and_email_have_one_semantic_home() {
    let codegen = read("crates/jet-codegen/src/Codegen/mod.rs");
    let core_calls = read("crates/jet-comptime/src/Comptime/Methods/core_calls.rs");
    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let url_mime = read("crates/jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");

    let kernel = "JetStd/Mime.rs";
    assert!(codegen.contains(kernel), "AOT must embed {kernel}");
    assert!(core_calls.contains(kernel), "comptime/interpreter must include {kernel}");
    assert!(
        codegen.contains("CoreLib/Email.rs"),
        "AOT must embed email kernel"
    );
    let jit = read("crates/jet-jit/src/Net.rs");
    assert!(
        read("crates/jet-comptime/src/Comptime/EmailAdapter.rs").contains("CoreLib/Email.rs")
    );
    assert!(jit.contains("CoreLib/Email.rs"));
    assert!(jit.contains("CoreLib/JetStd/UrlMime.rs"));

    assert!(
        !pure.contains("fn email_"),
        "comptime must not regain an email implementation"
    );
    assert!(!pure.contains("\"html\" | \"htm\" => Some(\"text/html\")"));
    assert!(url_mime.starts_with("    include!(\"Mime.rs\");"));
    assert!(!url_mime.contains("fn jet_mime_token"));
}

#[test]
fn sketches_have_one_semantic_home() {
    let kernel = "Core/Sketch.rs";
    for (path, tier) in [
        ("crates/jet-codegen/src/Codegen/mod.rs", "AOT"),
        ("crates/jet-jit/src/Sketch.rs", "JIT"),
        (
            "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            "comptime/interpreter",
        ),
    ] {
        assert!(read(path).contains(kernel), "{tier} must include {kernel}");
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let core = read("crates/jet-codegen/src/Prelude/Core.rs");
    let build = read("crates/jet-jit/build.rs");
    let jit = read("crates/jet-jit/src/Sketch.rs");
    for fingerprint in [
        "wrapping_mul(1099511628211)",
        "TDIGEST_DELTA",
        "x ^= x << 13",
    ] {
        assert!(!pure.contains(fingerprint), "comptime copied `{fingerprint}`");
        assert!(!core.contains(fingerprint), "Core.rs copied `{fingerprint}`");
        assert!(!jit.contains(fingerprint), "JIT copied `{fingerprint}`");
    }
    assert!(!build.contains("write_sketch_rt"));
}

#[test]
fn solver_has_one_semantic_home() {
    let kernel = "CoreLib/Top/Solver.rs";
    for (path, tier) in [
        ("crates/jet-codegen/src/Codegen/mod.rs", "AOT"),
        ("crates/jet-jit/src/Solver.rs", "JIT"),
        (
            "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            "comptime/interpreter",
        ),
    ] {
        assert!(read(path).contains(kernel), "{tier} must include {kernel}");
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let math_time = read("crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs");
    let jit = read("crates/jet-jit/src/Solver.rs");
    assert!(!pure.contains("failures + 1"));
    assert!(!math_time.contains("fn jet_solver_"));
    assert!(!jit.contains("solver.failures += 1"));
}

#[test]
fn civil_time_and_duration_have_one_semantic_home() {
    for (kernel, users) in [
        (
            "Core/Time.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-jit/src/Time.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "Core/Duration.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-jit/src/jit/runtime_host.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
    ] {
        for path in users {
            assert!(read(path).contains(kernel), "{path} must include {kernel}");
        }
    }
    for path in [
        "crates/jet-comptime/src/Comptime/Builtins.rs",
        "crates/jet-codegen/src/Codegen/TIR/eval/handles.rs",
    ] {
        assert!(
            read(path).contains("Core/Duration.rs"),
            "{path} must include the duration kernel"
        );
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let old_core = read("crates/jet-codegen/src/Prelude/Core.rs");
    let jit_time = read("crates/jet-jit/src/Time.rs");
    let jit_build = read("crates/jet-jit/build.rs");
    for fingerprint in [
        "365 * y + y / 4 - y / 100",
        "RFC3339 datetime needs Z or an offset",
        "value.checked_mul(scale)",
    ] {
        assert!(!pure.contains(fingerprint), "comptime copied `{fingerprint}`");
        assert!(!old_core.contains(fingerprint), "Core.rs copied `{fingerprint}`");
        assert!(!jit_time.contains(fingerprint), "JIT copied `{fingerprint}`");
    }
    assert!(!jit_build.contains("write_time_rt"));
}

#[test]
fn xml_has_one_semantic_home() {
    let kernel = "XmlKernel.rs";
    assert!(
        read("crates/jet-codegen/src/Codegen/mod.rs").contains(kernel),
        "AOT must embed {kernel}"
    );
    assert!(
        read("crates/jet-foundation/src/lib.rs").contains("pub mod XmlKernel"),
        "JIT and comptime must import the foundation XML kernel"
    );

    let comptime = read("crates/jet-comptime/src/Comptime/EncodingLite.rs");
    let comptime_production = comptime.split("#[cfg(test)]").next().unwrap();
    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let jit = read("crates/jet-jit/src/Encoding.rs");
    let aot = read("crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs");
    for operation in [
        "parse_document(",
        "parse_document_with(",
        "parse_document_bytes_with(",
        "render_document(",
        "render_document_bytes(",
        "canonical_document(",
        "document_root(",
        "expanded_name_parts(",
        "lookup_attribute(",
        "element_content(",
        "project_document_for_decode(",
    ] {
        assert!(aot.contains(&format!("jet_xml_kernel::{operation}")));
        assert!(
            !comptime_production.contains(&format!("XmlPull::{operation}")),
            "comptime bypassed XML kernel for `{operation}`"
        );
        assert!(
            !jit.contains(&format!("XmlPull::{operation}")),
            "JIT bypassed XML kernel for `{operation}`"
        );
    }
    assert!(!pure.contains("XmlPull::canonical_document("));
}

#[test]
fn measurement_has_one_semantic_home() {
    let kernel = "Core/Measurement.rs";
    for path in [
        "crates/jet-codegen/src/Codegen/mod.rs",
        "crates/jet-jit/src/jit/runtime_host.rs",
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
    ] {
        assert!(read(path).contains(kernel), "{path} must include {kernel}");
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let aot = read(
        "crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs",
    );
    let jit = read("crates/jet-jit/src/jit/runtime_host.rs");
    for fingerprint in [
        "left_uncertainty * left_uncertainty",
        "self.uncertainty * self.uncertainty",
        "jet_codegen::Comptime::Builtins::apply_method",
    ] {
        assert!(!pure.contains(fingerprint), "comptime copied `{fingerprint}`");
        assert!(!aot.contains(fingerprint), "AOT wrapper copied `{fingerprint}`");
        assert!(!jit.contains(fingerprint), "JIT wrapper copied `{fingerprint}`");
    }
}
