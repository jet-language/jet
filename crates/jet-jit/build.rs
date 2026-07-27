use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    write_yaml_std(&manifest);
    write_sketch_rt(&manifest);
    write_layout_rt(&manifest);
    write_reactive_rt(&manifest);
    write_time_rt(&manifest);
    write_regex_rt(&manifest);
}

fn write_reactive_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read ReactiveEventWatch.rs");
    // File is indented 4 spaces for jet_std string-concat embedding.
    let unindent = |s: &str| -> String {
        s.lines()
            .map(|line| line.strip_prefix("    ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let start = raw
        .find("// ── D-REACT1=B + D-DATARACE1=C")
        .expect("reactive marker");
    // Sync reactive + Event core, then skip AsyncEvent (needs task runtime),
    // then Hook/DecisionHook. JIT hosts async with thin adapters.
    let end_sync = raw
        .find("    pub struct JetAsyncPolicy")
        .expect("JetAsyncPolicy marker");
    let start_hooks = raw
        .find("    struct JetHookListener<")
        .expect("JetHookListener marker");
    let end_hooks = raw
        .find("    pub struct WatchHandle")
        .expect("WatchHandle marker");
    fn strip_orphan_derives(body: &mut String) {
        loop {
            let trimmed = body.trim_end();
            let orphan = trimmed.ends_with("#[derive(Clone)]")
                || trimmed.ends_with("#[derive(Clone, Copy)]")
                || trimmed.ends_with("#[derive(Clone, Copy, Debug, Eq, PartialEq)]")
                || trimmed.ends_with("#[derive(Clone, Copy, Debug, PartialEq, Eq)]");
            if !orphan {
                break;
            }
            if let Some(i) = body.rfind("#[derive") {
                body.truncate(i);
                *body = body.trim_end().to_string();
            } else {
                break;
            }
        }
    }
    let mut body = unindent(&raw[start..end_sync]);
    strip_orphan_derives(&mut body);
    body.push_str("\n");
    body.push_str(&unindent(&raw[start_hooks..end_hooks]));
    strip_orphan_derives(&mut body);
    // Pub types/fns JIT hosts need. Longest-first + already-pub skip so
    // `JetEvent` does not smash `JetEventScope` into `pub pub`.
    fn ensure_pub(body: &mut String, kind: &str, name: &str) {
        let bare = format!("{kind} {name}");
        let mut out = String::with_capacity(body.len() + 16);
        let bytes = body.as_str();
        let mut rest = bytes;
        while let Some(i) = rest.find(&bare) {
            let next = rest[i + bare.len()..].chars().next();
            let ident_continue = next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            out.push_str(&rest[..i]);
            if ident_continue {
                out.push_str(&bare);
            } else {
                let before = &rest[..i];
                let already = before.ends_with("pub ") || before.ends_with("pub(crate) ");
                if already {
                    out.push_str(&bare);
                } else {
                    out.push_str("pub ");
                    out.push_str(&bare);
                }
            }
            rest = &rest[i + bare.len()..];
        }
        out.push_str(rest);
        *body = out;
    }
    for name in [
        "JetReactiveEffect",
        "JetSignal",
        "JetDerived",
        "JetEventPolicy",
        "JetEventTrace",
        "JetEventScope",
        "JetEventOverflow",
        "JetEventConfigError",
        "JetSubscription",
        "JetEvent",
        "JetHookPolicy",
        "JetHookDecision",
        "JetHookOutcome",
        "JetHook",
        "JetDecisionHook",
        "JetAsyncEvent",
        "JetAsyncPolicy",
        "JetFailurePolicy",
        "JetDispatchReport",
        "JetDispatchState",
    ] {
        ensure_pub(&mut body, "struct", name);
        ensure_pub(&mut body, "enum", name);
    }
    for name in [
        "jet_reactive_effect_rooted",
        "jet_reactive_effect",
        "jet_reactive_scope",
    ] {
        ensure_pub(&mut body, "fn", name);
    }
    // Observe lives in Prelude::Observe; stub a no-op for the sync Event include.
    // AsyncEvent is host-shimmed (see Reactive.rs) — not included here.
    let stub = r#"
#[derive(Clone)]
pub struct JetObserveEvent {
    pub sequence: u64,
    pub source: &'static str,
    pub event_id: u64,
    pub owner_id: u64,
    pub subscription_id: u64,
    pub dispatch_id: u64,
    pub lifecycle: &'static str,
    pub queued: i64,
    pub blocked: i64,
    pub running: i64,
    pub capacity: i64,
    pub overflow: &'static str,
    pub priority: i64,
    pub failure: &'static str,
    pub terminal: &'static str,
}
pub fn jet_observe_event(_event: JetObserveEvent) {}
"#;
    body = body.replace("super::jet_observe_event", "jet_observe_event");
    body = body.replace("super::JetObserveEvent", "JetObserveEvent");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("reactive_rt.rs");
    std::fs::write(&out, format!("{stub}\n{body}\n")).expect("write reactive_rt.rs");
}

fn write_layout_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/Layout.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Prelude/Layout.rs");
    let start = raw
        .find("mod jet_layout {")
        .expect("jet_layout module in Layout.rs");
    let body = &raw[start + "mod jet_layout {".len()..];
    let body = body.trim_end();
    let body = body
        .strip_suffix('}')
        .expect("Layout.rs closing brace")
        .trim_end();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("layout_rt.rs");
    std::fs::write(&out, format!("{body}\n")).expect("write layout_rt.rs");
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
