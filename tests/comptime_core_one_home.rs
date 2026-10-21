//! D-ONCE-LAW1 guard for Core semantics shared with comptime.

mod common;

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
        "crates/jet-comptime/src/Comptime/Methods/core_calls/impure.rs",
    )));
    namespaces.extend(dispatched_namespaces(&read(
        "crates/jet-comptime/src/Comptime/CorePureParity.rs",
    )));
    // Solver construction enters through type-method dispatch rather than a
    // literal namespace arm, but `core.compute.solve` is still part of this registry.
    namespaces.insert("core.compute.solve".to_string());

    let architecture = read("Docs/spec/architecture.md");
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
    assert!(
        core_calls.contains(kernel),
        "comptime/interpreter must include {kernel}"
    );
    assert!(
        codegen.contains("CoreLib/Email.rs"),
        "AOT must embed email kernel"
    );
    assert!(read("crates/jet-comptime/src/Comptime/EmailAdapter.rs").contains("CoreLib/Email.rs"));

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
    // HLL/count-min/reservoir/TDigest are ordinary Jet carriers in Core/Data/sketch*.jet;
    // no tier may regain the deleted Rust kernel.
    let kernel = "Core/Sketch.rs";
    for path in [
        "crates/jet-codegen/src/Codegen/mod.rs",
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
        "Compiler/JetCodegen/Source/Emit/RuntimePrefix.jet",
    ] {
        assert!(!read(path).contains(kernel), "{path} must not include {kernel}");
    }
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/jet-codegen/src/Prelude/Core/Sketch.rs")
        .exists());
    for path in ["Core/Data/Sketch/Hll.jet", "Core/Data/Sketch/Cms.jet", "Core/Data/Sketch/Reservoir.jet"] {
        assert!(read(path).contains("#Test"), "{path} must cover its sketch policy");
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let core = read("crates/jet-codegen/src/Prelude/Core.rs");
    for fingerprint in [
        "wrapping_mul(1099511628211)",
        "TDIGEST_DELTA",
        "x ^= x << 13",
    ] {
        assert!(
            !pure.contains(fingerprint),
            "comptime copied `{fingerprint}`"
        );
        assert!(
            !core.contains(fingerprint),
            "Core.rs copied `{fingerprint}`"
        );
    }
}

#[test]
fn solver_has_one_semantic_home() {
    let source = read("Core/Compute/Solve.jet");
    assert!(source.contains("self.checked += 1"));
    assert!(source.contains("if !ok -> self.failures += 1"));
    assert!(source.contains("fn __show(self)"));
    for path in [
        "crates/jet-codegen/src/Codegen/mod.rs",
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
    ] {
        assert!(!read(path).contains("CoreLib/Top/Solver.rs"));
    }
    let mir = read("crates/jet-codegen/src/Codegen/MIREval.rs");
    assert!(mir.contains("jet_foundation::MIR::mir_source_value_hook"));
    let comptime = read("crates/jet-comptime/src/Comptime/Methods/dispatch.rs");
    assert!(comptime.contains("CoreModuleExports::core_source_value_hook"));
}

#[test]
fn civil_time_and_duration_have_one_semantic_home() {
    for (kernel, users) in [
        (
            "Core/Time.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "Core/Duration.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
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
        "crates/jet-codegen/src/Codegen/MIREval.rs",
    ] {
        assert!(
            read(path).contains("Core/Duration.rs"),
            "{path} must include the duration kernel"
        );
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let old_core = read("crates/jet-codegen/src/Prelude/Core.rs");
    for fingerprint in [
        "365 * y + y / 4 - y / 100",
        "RFC3339 datetime needs Z or an offset",
        "value.checked_mul(scale)",
    ] {
        assert!(
            !pure.contains(fingerprint),
            "comptime copied `{fingerprint}`"
        );
        assert!(
            !old_core.contains(fingerprint),
            "Core.rs copied `{fingerprint}`"
        );
    }
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
        "comptime must import the foundation XML kernel"
    );

    let comptime = read("crates/jet-comptime/src/Comptime/EncodingLite.rs");
    let comptime_production = comptime.split("#[cfg(test)]").next().unwrap();
    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let aot = read("crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs");
    for (tier, source, call) in [
        ("AOT", aot.as_str(), "jet_xml_kernel::parse_document(text)"),
        (
            "comptime/interpreter",
            comptime_production,
            "XmlKernel::parse_document(text)",
        ),
    ] {
        assert!(
            source.contains(call),
            "{tier} must pass XML source unchanged to the shared kernel"
        );
    }
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
    }
    assert!(!pure.contains("XmlPull::canonical_document("));
}

#[test]
fn measurement_has_one_semantic_home() {
    let source = read("Core/Units/Units.jet");
    assert!(source.contains("struct Measurement<T>"));
    for function in ["new", "from_relative", "add", "sub", "mul", "div", "sqrt", "show"] {
        assert!(source.contains(&format!("fn __measurement_{function}(")));
    }
    for path in [
        "crates/jet-codegen/src/Codegen/mod.rs",
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
    ] {
        assert!(!read(path).contains("Core/Measurement.rs"), "{path} retained the native kernel");
    }

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let aot = read("crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs");
    for fingerprint in [
        "left_uncertainty * left_uncertainty",
        "self.uncertainty * self.uncertainty",
        "jet_codegen::Comptime::Builtins::apply_method",
    ] {
        assert!(
            !pure.contains(fingerprint),
            "comptime copied `{fingerprint}`"
        );
        assert!(
            !aot.contains(fingerprint),
            "AOT wrapper copied `{fingerprint}`"
        );
    }

    let docs = read("Docs/spec/reference/core-library.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for statement in [
        "first-order linear propagation with uncorrelated inputs",
        "Correlated errors are out of scope",
    ] {
        assert!(
            docs.contains(statement),
            "measurement docs omit `{statement}`"
        );
    }
}

#[test]
fn round_three_value_kernels_have_one_semantic_home() {
    for (kernel, users) in [
        (
            "Core/Fmt.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "Core/Path.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "Core/SeededRandom.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/dispatch.rs",
            ],
        ),
        (
            "Core/NetPure.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "CoreLib/Top/MathLibPure.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "Core/Loadable.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            ],
        ),
        (
            "CoreLib/Top/Text.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/TextLite.rs",
            ],
        ),
        (
            "CoreLib/JetStd/UrlMime.rs",
            [
                "crates/jet-codegen/src/Codegen/mod.rs",
                "crates/jet-comptime/src/Comptime/UrlLite.rs",
            ],
        ),
    ] {
        for path in users {
            assert!(read(path).contains(kernel), "{path} must include {kernel}");
        }
    }

    let codecs = read("crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs");
    assert!(!codecs.contains("fn jet_std_hex_encode"));
    assert!(!codecs.contains("fn jet_std_b64_encode"));
    assert!(!codecs.contains("fn jet_std_base32_encode"));
    let kernel = read("crates/jet-codegen/src/Prelude/Core/EncodingBase.rs");
    for name in [
        "jet_std_hex_encode", "jet_std_hex_decode", "jet_std_hex_dump",
        "jet_std_crc_hqx", "jet_std_crc32", "jet_std_b64_encode",
        "jet_std_base32_encode", "jet_std_base32hex_encode",
    ] {
        assert!(!kernel.contains(name), "{name} must live in Jet Core");
    }
    for path in ["Core/Encoding/Hex.jet", "Core/Encoding/Base64.jet", "Core/Encoding/Base32.jet"] {
        assert!(read(path).contains("#Test"), "{path} must cover its codec kernels");
    }
    let core_calls = read("crates/jet-comptime/src/Comptime/Methods/core_calls.rs");
    let plain_calls = read("crates/jet-comptime/src/Comptime/Methods/core_calls/plain_calls.rs");
    for decoder in [
        "base_encoding_dispatch::decode_base64",
        "base_encoding_dispatch::decode_base64url",
        "base_encoding_dispatch::decode_base32",
    ] {
        assert!(plain_calls.contains(decoder), "comptime must use {decoder}");
    }

    let data_fmt = read("crates/jet-codegen/src/Prelude/CoreLib/Top/DataFmt.rs");
    assert!(!data_fmt.contains("fn jet_fmt_number"));

    assert!(read("crates/jet-comptime/src/Comptime/Builtins.rs").contains("Core/Loadable.rs"));

    let text = read("crates/jet-comptime/src/Comptime/TextLite.rs");
    assert!(!text.contains("fn nfd_inner"));
    assert!(!text.contains("fn ccc("));

    let url = read("crates/jet-comptime/src/Comptime/UrlLite.rs");
    assert!(!url.contains("fn url_valid_scheme"));

    let pure = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    assert!(!pure.contains("parse::<std::net::IpAddr>"));
    assert!(!pure.contains("parse::<std::net::SocketAddr>"));

    assert!(!core_calls.contains("std::path::Path::new(a)"));
    assert!(!core_calls.contains("fn comma_int_ct"));
}
