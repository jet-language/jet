//! Compiler phase timing (c121).
//!
//! A zero-dependency stopwatch the pipeline laps once per phase. Timing is
//! collected only when the `JET_TIMING` env var is set, so normal builds pay
//! nothing. JSON is hand-rolled — no serde, no external crate (I6).

use jet_foundation::JSON::json_escape;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

const TIMING_SCHEMA: &str = "jet.compiler-timing";
const TIMING_VERSION: u32 = 1;
const FULL_ARTIFACT_NAME: &str = "jet-timing.full.json";
const MAX_CONTEXT_BYTES: usize = 512;

fn bounded_text(value: String) -> String {
    if value.len() <= MAX_CONTEXT_BYTES {
        return value;
    }
    let mut end = MAX_CONTEXT_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn environment_text(key: &str) -> Option<String> {
    std::env::var_os(key)
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .map(bounded_text)
}

fn source_name(value: &str) -> String {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| bounded_text(value.to_owned()))
}

fn command_name() -> Option<String> {
    std::env::args_os()
        .nth(1)
        .map(|value| value.to_string_lossy().into_owned())
}

fn command_profile() -> String {
    for value in std::env::args_os().skip(1) {
        let value = value.to_string_lossy();
        if value == "--release" {
            return "release".into();
        }
        if value == "--small" {
            return "small".into();
        }
        if let Some(profile) = value.strip_prefix("--profile=") {
            if !profile.is_empty() {
                return bounded_text(profile.to_owned());
            }
        }
    }
    match command_name().as_deref() {
        Some("run") | Some("dev") => "fast".into(),
        Some("build") => "default".into(),
        _ => "unknown".into(),
    }
}

fn command_backend() -> &'static str {
    match command_name().as_deref() {
        Some("run") | Some("dev") => "cranelift-jit",
        Some("build") => "rustc-aot",
        _ => "unknown",
    }
}

fn default_source() -> String {
    if let Some(source) = environment_text("JET_TIMING_SOURCE") {
        return source_name(&source);
    }
    std::env::args_os()
        .skip(1)
        .find_map(|value| {
            let value = value.to_string_lossy().into_owned();
            value.ends_with(".jet").then(|| source_name(&value))
        })
        .unwrap_or_else(|| "unknown".into())
}

fn default_environment() -> String {
    bounded_text(format!(
        "os={};arch={};target={};profile={};backend={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        option_env!("JET_BUILD_TARGET").unwrap_or("unknown"),
        command_profile(),
        command_backend()
    ))
}

/// Whether phase timing is requested for this process. Set `JET_TIMING=1`.
pub fn enabled() -> bool {
    timing_requested(std::env::var("JET_TIMING").ok().as_deref())
}

fn timing_requested(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Directory that must receive `jet-timing.json` for a compile-latency probe.
pub fn output_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("JET_TIMING_DIR").map(std::path::PathBuf::from)
}

/// A running stopwatch that records the elapsed time of each phase as it is
/// lapped. Durations are measured from the previous lap (or construction).
pub struct PhaseTimer {
    phases: Vec<(String, u128)>,
    last: Instant,
    source: Option<String>,
    environment: Option<String>,
}

impl Default for PhaseTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl PhaseTimer {
    pub fn new() -> Self {
        PhaseTimer {
            phases: Vec::new(),
            last: Instant::now(),
            source: None,
            environment: None,
        }
    }

    /// Construct a timer with stable source and environment labels.
    ///
    /// Normal compiler entry points infer these labels from their command and
    /// arguments. Probes and embedders can provide the exact logical labels
    /// without putting absolute paths or host-specific values in the report.
    pub fn with_context(source: impl AsRef<str>, environment: impl AsRef<str>) -> Self {
        let mut timer = Self::new();
        timer.set_source(source);
        timer.set_environment(environment);
        timer
    }

    pub fn set_source(&mut self, source: impl AsRef<str>) {
        let source = bounded_text(source.as_ref().to_owned());
        self.source = Some(source_name(&source));
    }

    pub fn set_environment(&mut self, environment: impl AsRef<str>) {
        self.environment = Some(bounded_text(environment.as_ref().to_owned()));
    }

    /// Record the time since the previous lap under `phase`, in microseconds,
    /// and reset the lap clock.
    pub fn lap(&mut self, phase: &str) {
        let now = Instant::now();
        let us = now.duration_since(self.last).as_micros();
        self.phases.push((phase.to_string(), us));
        self.last = now;
    }

    /// Record a phase measured by a nested producer that owns its boundary.
    /// The shared stopwatch remains at its current boundary; this keeps nested
    /// compiler seams from double-counting their wall interval.
    pub fn record_us(&mut self, phase: &str, us: u128) {
        self.phases.push((phase.to_string(), us));
    }

    /// Record a non-time scalar metric (e.g. generated-Rust byte count) so it
    /// rides along in the same report. Stored verbatim, not as a duration.
    pub fn metric(&mut self, name: &str, value: u128) {
        self.phases.push((name.to_string(), value));
    }

    /// Record one cache hit in the explanation.
    pub fn cache_hit(&mut self) {
        self.metric("cache_hit", 1);
    }

    /// Record one cache miss in the explanation.
    pub fn cache_miss(&mut self) {
        self.metric("cache_miss", 1);
    }

    pub fn report(&self) -> &[(String, u128)] {
        &self.phases
    }

    fn phase_totals(&self) -> Vec<(String, u128)> {
        let mut totals = BTreeMap::<String, u128>::new();
        for (name, value) in &self.phases {
            let total = totals.entry(name.clone()).or_default();
            *total = total.saturating_add(*value);
        }
        totals.into_iter().collect()
    }

    fn cache_counts(&self) -> (u128, u128) {
        let mut hits = 0u128;
        let mut misses = 0u128;
        for (name, value) in &self.phases {
            match name.as_str() {
                "cache_hit" | "cache_hits" => hits = hits.saturating_add(*value),
                "cache_miss" | "cache_misses" => misses = misses.saturating_add(*value),
                _ => {}
            }
        }
        (hits, misses)
    }

    fn top_cause(phases: &[(String, u128)]) -> String {
        let mut top: Option<(String, u128)> = None;
        for (name, value) in phases {
            if name.starts_with("cache_")
                || matches!(name.as_str(), "rust_bytes" | "binary_bytes")
                || *value == 0
            {
                continue;
            }
            if top
                .as_ref()
                .is_none_or(|(_, previous)| *value > *previous)
            {
                top = Some((name.clone(), *value));
            }
        }
        top.map_or_else(
            || "unavailable".into(),
            |(name, value)| format!("{name}={value}us"),
        )
    }

    fn phase_array(phases: &[(String, u128)]) -> String {
        let mut s = String::from("[");
        for (i, (name, us)) in phases.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"name\":\"{}\",\"us\":{}}}",
                json_escape(name),
                us
            ));
        }
        s.push(']');
        s
    }

    fn explanation_json(&self, full: bool) -> String {
        let totals = self.phase_totals();
        let phases = if full { &self.phases } else { &totals };
        let (cache_hits, cache_misses) = self.cache_counts();
        let source = self
            .source
            .as_deref()
            .map_or_else(default_source, str::to_owned);
        let environment = self
            .environment
            .as_deref()
            .map_or_else(default_environment, str::to_owned);
        let mut s = String::from("{\"phases\":");
        s.push_str(&Self::phase_array(phases));
        s.push_str(",\"phase_totals\":");
        s.push_str(&Self::phase_array(&totals));
        s.push_str(&format!(
            ",\"schema\":\"{}\",\"version\":{},\"source\":\"{}\",\"top_cause\":\"{}\",\"cache_hits\":{},\"cache_misses\":{},\"environment\":\"{}\",\"full_artifact\":\"{}\"",
            TIMING_SCHEMA,
            TIMING_VERSION,
            json_escape(&source),
            json_escape(&Self::top_cause(&totals)),
            cache_hits,
            cache_misses,
            json_escape(&environment),
            FULL_ARTIFACT_NAME,
        ));
        if full {
            s.push_str(",\"detail\":\"full\"");
        }
        s.push_str("}\n");
        s
    }

    /// Hand-rolled stable JSON for the bounded compiler explanation.
    pub fn to_json(&self) -> String {
        self.explanation_json(false)
    }

    /// Hand-rolled stable JSON retaining both raw phase events and totals.
    pub fn to_full_json(&self) -> String {
        self.explanation_json(true)
    }

    /// Write the bounded explanation and its explicit full artifact.
    ///
    /// Ordinary `JET_TIMING=1` builds stay best-effort so timing never breaks
    /// a build. A compile-latency probe sets `JET_TIMING_DIR` to the scratch
    /// tree it will read; that write is required and also lands in CWD so a
    /// `project_root` that is not the process directory cannot hide the file.
    pub fn write_to(&self, dir: &Path) {
        let json = self.to_json();
        let full = self.to_full_json();
        write_artifacts(dir, &json, &full);
        if let Ok(cwd) = std::env::current_dir() {
            if cwd != dir {
                write_artifacts(&cwd, &json, &full);
            }
        }
        if let Some(out) = output_dir() {
            let _ = std::fs::create_dir_all(&out);
            write_artifacts(&out, &json, &full);
        }
    }
}

fn write_artifacts(dir: &Path, json: &str, full: &str) {
    let _ = std::fs::write(dir.join("jet-timing.json"), json);
    let _ = std::fs::write(dir.join(FULL_ARTIFACT_NAME), full);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape() {
        let mut t = PhaseTimer::new();
        t.lap("lex");
        t.metric("rust_bytes", 4096);
        let j = t.to_json();
        assert!(j.starts_with("{\"phases\":["));
        assert!(j.contains("\"name\":\"lex\""));
        assert!(j.contains("\"name\":\"rust_bytes\",\"us\":4096"));
        assert!(j.trim_end().ends_with('}'));
    }

    #[test]
    fn explanation_json_is_stable_and_actionable() {
        let mut timer = PhaseTimer::with_context("/scratch/src/main.jet", "linux/amd64/test");
        timer.record_us("sema", 9);
        timer.record_us("parse", 3);
        timer.cache_hit();
        timer.cache_miss();
        assert_eq!(
            timer.to_json(),
            "{\"phases\":[{\"name\":\"cache_hit\",\"us\":1},{\"name\":\"cache_miss\",\"us\":1},{\"name\":\"parse\",\"us\":3},{\"name\":\"sema\",\"us\":9}],\"phase_totals\":[{\"name\":\"cache_hit\",\"us\":1},{\"name\":\"cache_miss\",\"us\":1},{\"name\":\"parse\",\"us\":3},{\"name\":\"sema\",\"us\":9}],\"schema\":\"jet.compiler-timing\",\"version\":1,\"source\":\"main.jet\",\"top_cause\":\"sema=9us\",\"cache_hits\":1,\"cache_misses\":1,\"environment\":\"linux/amd64/test\",\"full_artifact\":\"jet-timing.full.json\"}\n"
        );
        assert!(timer.to_full_json().contains("\"detail\":\"full\""));
    }

    #[test]
    fn full_artifact_writer_publishes_explicit_deep_report() {
        let dir = std::env::temp_dir().join(format!(
            "jet-phase-timing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let timer = PhaseTimer::with_context("main.jet", "test");
        write_artifacts(&dir, &timer.to_json(), &timer.to_full_json());
        assert!(dir.join("jet-timing.json").is_file());
        assert!(dir.join(FULL_ARTIFACT_NAME).is_file());
        assert!(std::fs::read_to_string(dir.join(FULL_ARTIFACT_NAME))
            .unwrap()
            .contains("\"detail\":\"full\""));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timing_flag_accepts_only_one() {
        assert!(timing_requested(Some("1")));
        assert!(!timing_requested(Some("0")));
        assert!(!timing_requested(Some("true")));
        assert!(!timing_requested(None));
    }
}
