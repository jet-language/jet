//! Compiler phase timing (c121).
//!
//! A zero-dependency stopwatch the pipeline laps once per phase. Timing is
//! collected only when the `JET_TIMING` env var is set, so normal builds pay
//! nothing. JSON is hand-rolled — no serde, no external crate (I6).

use std::time::Instant;

/// Whether phase timing is requested for this process. Set `JET_TIMING=1`.
pub fn enabled() -> bool {
    std::env::var_os("JET_TIMING").is_some()
}

/// A running stopwatch that records the elapsed time of each phase as it is
/// lapped. Durations are measured from the previous lap (or construction).
pub struct PhaseTimer {
    phases: Vec<(String, u128)>,
    last: Instant,
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
        }
    }

    /// Record the time since the previous lap under `phase`, in microseconds,
    /// and reset the lap clock.
    pub fn lap(&mut self, phase: &str) {
        let now = Instant::now();
        let us = now.duration_since(self.last).as_micros();
        self.phases.push((phase.to_string(), us));
        self.last = now;
    }

    /// Record a non-time scalar metric (e.g. generated-Rust byte count) so it
    /// rides along in the same report. Stored verbatim, not as a duration.
    pub fn metric(&mut self, name: &str, value: u128) {
        self.phases.push((name.to_string(), value));
    }

    pub fn report(&self) -> &[(String, u128)] {
        &self.phases
    }

    /// Hand-rolled JSON: `{"phases":[{"name":"sema","us":1234},...]}`.
    pub fn to_json(&self) -> String {
        let mut s = String::from("{\"phases\":[");
        for (i, (name, us)) in self.phases.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{{\"name\":\"{}\",\"us\":{}}}", escape(name), us));
        }
        s.push_str("]}\n");
        s
    }

    /// Write the report to `<dir>/jet-timing.json`. Best-effort: a write
    /// failure is silently ignored, since timing must never break a build.
    pub fn write_to(&self, dir: &std::path::Path) {
        let _ = std::fs::write(dir.join("jet-timing.json"), self.to_json());
    }
}

/// Escape a phase name for embedding in a JSON string. Phase names are
/// compiler-internal ASCII identifiers, but stay correct regardless.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
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
        assert!(j.trim_end().ends_with("]}"));
    }
}
