//! Card #392 pass 3: `core.data`'s fixed-signature stats + plot surface
//! (D-DATA-SURFACE1/PLOT1/STATUS1 — `sum`/`mean`/`min`/`max`/`median`/
//! `variance`/`stddev`/`quantile`/`rolling_mean`/`describe`/`status`/
//! `bar_text`/`bar_svg`), ported verbatim from AOT's `jet_data_*` /
//! `jet_data_svg_escape` (`crates/jet-codegen/src/Prelude/CoreLib/Top/
//! EncodingTraits.rs` + `DataFmt.rs`) so comptime/REPL tier-0 matches AOT
//! byte-for-byte (R12 parity).
//!
//! Not here: none of the call-site-typed table/lazy pipeline names above —
//! those live in `DataPipeline.rs` (including `schema`).

#[allow(unused_imports)]
use jet_foundation::Outcome::*;

mod data_plot_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    pub(crate) mod jet_std {
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;
        #[derive(Clone, Debug)]
        pub(crate) struct DataGroup {
            pub(crate) key: String,
            pub(crate) count: i64,
            pub(crate) sum: f64,
            pub(crate) mean: f64,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataLineOptions {
            pub(crate) title: String,
            pub(crate) x_label: String,
            pub(crate) y_label: String,
            pub(crate) markers: bool,
            pub(crate) reference: JetOutcome<f64, JetAbsent>,
            pub(crate) style: String,
            pub(crate) color: String,
            pub(crate) legend: String,
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/DataPlot.rs");
}

pub(super) fn sum(values: &[f64]) -> f64 {
    values.iter().copied().sum()
}

pub(super) fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        sum(values) / values.len() as f64
    }
}

pub(super) fn min(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

pub(super) fn max(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

pub(super) fn median(values: &[f64]) -> f64 {
    quantile(values, 0.5)
}

pub(super) fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    }
}

pub(super) fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let m = mean(values);
    values
        .iter()
        .map(|v| {
            let d = *v - m;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64
}

pub(super) fn stddev(values: &[f64]) -> f64 {
    variance(values).sqrt()
}

pub(super) fn rolling_mean(values: &[f64], width: i64) -> Vec<f64> {
    let width = width.max(1) as usize;
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &values[start..=i];
        out.push(window.iter().sum::<f64>() / window.len() as f64);
    }
    out
}

/// Full DataStatus rows — mirrors `jet_data_status` exactly
/// `(step, path, copy, ownership, trust, fallback, replacement)`.
fn bridge_tool_on_path(tool: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(tool).is_file())
}

fn bridge_path(step: &str) -> &'static str {
    match step {
        "r.*"
            if bridge_tool_on_path("Rscript")
                && std::env::var("JET_DATA_R_BRIDGE").as_deref() == Ok("1") =>
        {
            "available"
        }
        _ => "unavailable",
    }
}

pub(super) fn status_rows(
) -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    vec![
        (
            "core.data.csv",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.stats",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.table",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.lazy",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.missing",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.schema",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "core.data.json",
            "native",
            "none",
            "jet",
            "native",
            "none",
            "native",
        ),
        (
            "py.*",
            bridge_path("py.*"),
            "owned-copy",
            "python-sidecar",
            "untrusted-foreign",
            "none",
            "core.data native table/series/stats",
        ),
        (
            "r.*",
            bridge_path("r.*"),
            "owned-copy",
            "r-sidecar",
            "untrusted-foreign",
            "none",
            "core.data.Table typed round-trip",
        ),
        (
            "gpu.*",
            bridge_path("gpu.*"),
            "device-transfer",
            "device-buffer",
            "untrusted-accelerator",
            "none",
            "core.data / Tensor native CPU path",
        ),
    ]
}

pub(super) fn normalize_bridge_provider(provider: &str) -> Option<&'static str> {
    let lower = provider.trim().trim_end_matches('.').to_ascii_lowercase();
    let p = lower.strip_suffix(".*").unwrap_or(lower.as_str());
    match p {
        "py" | "python" => Some("py.*"),
        "r" => Some("r.*"),
        "gpu" | "cuda" | "metal" | "vulkan" | "webgpu" => Some("gpu.*"),
        _ => None,
    }
}

/// `(key, count)` pairs — the `bar_text`/`bar_svg` renderers only read
/// `DataGroup.key`/`.count` (never `.sum`/`.mean`), matching AOT.
pub(super) fn bar_text(groups: &[(String, i64)]) -> String {
    let mut lines = Vec::new();
    for (key, count) in groups {
        let n = if *count < 0 { 0 } else { (*count).min(40) } as usize;
        lines.push(format!("{} | {} {}", key, "#".repeat(n), count));
    }
    lines.join("\n")
}

pub(super) fn bar_svg(groups: &[(String, i64)]) -> String {
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64, height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, (key, count)) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((*count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            svg_escape(key)
        ));
        out.push_str(&format!(
            "<rect x=\"96\" y=\"{}\" width=\"{}\" height=\"14\" fill=\"#2f6f73\"/>",
            (y - 12.0) as i64,
            bar_w as i64
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            (104.0 + bar_w) as i64,
            y as i64,
            count
        ));
    }
    out.push_str("</svg>");
    out
}

pub(super) struct LineOptions {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub markers: bool,
    pub reference: Option<f64>,
    pub style: String,
    pub color: String,
    pub legend: String,
}

pub(super) struct LineError {
    pub(super) kind: &'static str,
    pub(super) operation: &'static str,
    pub(super) reason: &'static str,
    pub(super) index: Option<i64>,
}

pub(super) fn line_text(groups: &[(String, f64)], options: &LineOptions) -> String {
    let groups = groups
        .iter()
        .map(|(key, value)| data_plot_rt::jet_std::DataGroup {
            key: key.clone(),
            count: 1,
            sum: *value,
            mean: *value,
        })
        .collect::<Vec<_>>();
    let options = data_plot_rt::jet_std::DataLineOptions {
        title: options.title.clone(),
        x_label: options.x_label.clone(),
        y_label: options.y_label.clone(),
        markers: options.markers,
        reference: jet_outcome_of(options.reference),
        style: options.style.clone(),
        color: options.color.clone(),
        legend: options.legend.clone(),
    };
    data_plot_rt::jet_data_line_text(&groups, &options)
}

pub(super) fn line_svg(groups: &[(String, f64)], options: &LineOptions) -> String {
    let groups = groups
        .iter()
        .map(|(key, value)| data_plot_rt::jet_std::DataGroup {
            key: key.clone(),
            count: 1,
            sum: *value,
            mean: *value,
        })
        .collect::<Vec<_>>();
    let options = data_plot_rt::jet_std::DataLineOptions {
        title: options.title.clone(),
        x_label: options.x_label.clone(),
        y_label: options.y_label.clone(),
        markers: options.markers,
        reference: jet_outcome_of(options.reference),
        style: options.style.clone(),
        color: options.color.clone(),
        legend: options.legend.clone(),
    };
    data_plot_rt::jet_data_line_svg(&groups, &options)
}

fn line_error(error: data_plot_rt::DataPlotError) -> LineError {
    LineError {
        kind: error.kind,
        operation: error.operation,
        reason: error.reason,
        index: error.index,
    }
}

pub(super) fn line_text_checked(
    groups: &[(String, f64)],
    options: &LineOptions,
) -> Result<String, LineError> {
    let groups = groups
        .iter()
        .map(|(key, value)| data_plot_rt::jet_std::DataGroup {
            key: key.clone(),
            count: 1,
            sum: *value,
            mean: *value,
        })
        .collect::<Vec<_>>();
    let options = data_plot_rt::jet_std::DataLineOptions {
        title: options.title.clone(),
        x_label: options.x_label.clone(),
        y_label: options.y_label.clone(),
        markers: options.markers,
        reference: jet_outcome_of(options.reference),
        style: options.style.clone(),
        color: options.color.clone(),
        legend: options.legend.clone(),
    };
    data_plot_rt::jet_data_line_text_plot_checked(&groups, &options).map_err(line_error)
}

pub(super) fn line_svg_checked(
    groups: &[(String, f64)],
    options: &LineOptions,
) -> Result<String, LineError> {
    let groups = groups
        .iter()
        .map(|(key, value)| data_plot_rt::jet_std::DataGroup {
            key: key.clone(),
            count: 1,
            sum: *value,
            mean: *value,
        })
        .collect::<Vec<_>>();
    let options = data_plot_rt::jet_std::DataLineOptions {
        title: options.title.clone(),
        x_label: options.x_label.clone(),
        y_label: options.y_label.clone(),
        markers: options.markers,
        reference: jet_outcome_of(options.reference),
        style: options.style.clone(),
        color: options.color.clone(),
        legend: options.legend.clone(),
    };
    data_plot_rt::jet_data_line_svg_plot_checked(&groups, &options).map_err(line_error)
}

fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// `bar_text`/`bar_svg` take `[DataGroup]`, but `DataGroup` isn't a
// user-constructible type at comptime/REPL yet (only `group_count`/
// `group_sum`/`group_mean` — the still-open generic pipeline gap — produce
// one), so there's no Jet-source transcript to exercise them with; these
// unit tests check the ported function directly against AOT's exact
// literal output (`jet_data_bar_text`/`_svg` in `DataFmt.rs`) instead.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_text_matches_aot_shape() {
        let groups = vec![("a".to_string(), 3), ("b".to_string(), 1)];
        assert_eq!(bar_text(&groups), "a | ### 3\nb | # 1");
    }

    #[test]
    fn bar_text_clamps_negative_and_over_40() {
        let groups = vec![("neg".to_string(), -5), ("big".to_string(), 100)];
        assert_eq!(
            bar_text(&groups),
            format!("neg |  -5\nbig | {} 100", "#".repeat(40))
        );
    }

    #[test]
    fn bar_svg_matches_aot_shape() {
        let groups = vec![("a".to_string(), 2)];
        let svg = bar_svg(&groups);
        assert!(svg.starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"48\""
        ));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(">a</text>"));
        assert!(svg.contains(">2</text>"));
    }

    #[test]
    fn svg_escape_matches_aot() {
        assert_eq!(
            svg_escape("<a & \"b\">"),
            "&lt;a &amp; &quot;b&quot;&gt;"
        );
    }

    #[test]
    fn stats_match_aot_formulas() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(sum(&xs), 10.0);
        assert_eq!(mean(&xs), 2.5);
        assert_eq!(min(&xs), 1.0);
        assert_eq!(max(&xs), 4.0);
        assert_eq!(median(&xs), 2.5);
        assert_eq!(variance(&xs), 1.25);
        assert_eq!(stddev(&xs), 1.25f64.sqrt());
        assert_eq!(quantile(&xs, 0.25), 1.75);
        assert_eq!(rolling_mean(&xs, 2), vec![1.0, 1.5, 2.5, 3.5]);
    }

    #[test]
    fn empty_stats_are_zero_not_panics() {
        let xs: [f64; 0] = [];
        assert_eq!(sum(&xs), 0.0);
        assert_eq!(mean(&xs), 0.0);
        assert_eq!(min(&xs), 0.0);
        assert_eq!(max(&xs), 0.0);
        assert_eq!(median(&xs), 0.0);
        assert_eq!(variance(&xs), 0.0);
    }
}
