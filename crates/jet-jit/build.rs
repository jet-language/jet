use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    write_yaml_std(&manifest);
    write_sketch_rt(&manifest);
    write_time_rt(&manifest);
    write_regex_rt(&manifest);
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

fn write_time_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/Core.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Prelude/Core.rs");
    let end = raw
        .find("\n// D-PARCAPTURE1")
        .expect("D-PARCAPTURE1 marker in Core.rs");
    let mut body = raw[..end].to_string();
    // JetShow / JetDebug are AOT-prelude-only.
    while let Some(impl_at) = body.find("impl JetShow for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n}\n")
            .map(|i| i + 3)
            .expect("JetShow impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    while let Some(impl_at) = body.find("impl JetDebug for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n}\n")
            .map(|i| i + 3)
            .expect("JetDebug impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    for name in [
        "JetZonedDateTime",
        "JetDateTime",
        "JetLocalTime",
        "JetPeriod",
        "JetInstant",
        "JetTtInfo",
        "JetZone",
        "JetDate",
    ] {
        let from = format!("struct {name} {{");
        let to = format!("pub(crate) struct {name} {{");
        body = body.replace(&from, &to);
    }
    for method in [
        "new",
        "today_utc",
        "parse",
        "year",
        "month",
        "day",
        "to_string_fmt",
        "format_pattern",
        "add_days",
        "add_months",
        "add_period",
        "diff_days",
        "weekday",
        "day_of_year",
        "iso_weekday",
        "iso_week",
        "from_timestamp",
        "to_timestamp",
        "date",
        "hour",
        "minute",
        "second",
        "now",
        "parse_rfc3339",
        "format_rfc3339",
        "to_unix_ms",
        "from_unix_ms",
        "elapsed_millis",
        "utc",
        "named",
        "in_zone",
        "offset_seconds",
        "from_local",
        "days",
        "months",
        "years",
        "plus_duration_ms",
    ] {
        let from = format!("    fn {method}(");
        let to = format!("    pub(crate) fn {method}(");
        body = body.replace(&from, &to);
    }
    for free in [
        "jet_time_utc_from_parts",
        "jet_time_offset_string",
        "jet_time_format_pattern",
    ] {
        body = body.replace(&format!("fn {free}("), &format!("pub(crate) fn {free}("));
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("time_rt.rs");
    std::fs::write(&out, body).expect("write time_rt.rs");
}

fn write_regex_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Open.rs");
    let start = raw
        .find("    #[derive(Clone, Debug)]\n    pub struct RegexFlags {")
        .expect("RegexFlags derive in Open.rs");
    let mut body = raw[start..].to_string();
    while let Some(impl_at) = body.find("impl crate::JetShow for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n    }\n")
            .map(|i| i + 6)
            .expect("JetShow impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    while let Some(impl_at) = body.find("impl crate::JetDebug for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n    }\n")
            .map(|i| i + 6)
            .expect("JetDebug impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("regex_rt.rs");
    std::fs::write(&out, body).expect("write regex_rt.rs");
}
